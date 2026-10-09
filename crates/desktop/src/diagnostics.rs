//! Local diagnostics deliberately exclude command arguments, queries, credentials,
//! library contents and arbitrary log/panic messages. Stack traces can contain paths.
use std::{
    backtrace::Backtrace,
    fs::{self, File, OpenOptions},
    io::Write,
    path::Path,
    sync::{Mutex, OnceLock},
};

const MAXIMUM_LOG_BYTES: u64 = 2 * 1024 * 1024;
const RETAINED_LOGS: usize = 10;
static LOGGER: OnceLock<SessionLogger> = OnceLock::new();

struct SessionLogger {
    file: Mutex<File>,
}

pub(crate) struct Session;

impl Drop for Session {
    fn drop(&mut self) {
        record("session ended normally");
    }
}

pub(crate) fn initialize() -> Option<Session> {
    let directory = startpoint_core::application_paths()
        .ok()?
        .data_directory
        .join("diagnostics");
    initialize_at(&directory)
}

fn initialize_at(directory: &Path) -> Option<Session> {
    fs::create_dir_all(&directory).ok()?;
    prune_logs(&directory);
    let name = format!(
        "session-{}-{}.log",
        chrono::Utc::now().format("%Y%m%dT%H%M%S%.3fZ"),
        std::process::id()
    );
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(directory.join(name))
        .ok()?;
    LOGGER
        .set(SessionLogger {
            file: Mutex::new(file),
        })
        .ok()?;
    if let Some(logger) = LOGGER.get() {
        let _ = log::set_logger(logger);
        log::set_max_level(log::LevelFilter::Warn);
    }
    record(&format!(
        "Startpoint {} {}-{} pid={}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::process::id()
    ));
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        // Capture before Rust aborts on a panic crossing a native window callback.
        // Do not format the payload: third-party panics can echo secrets/user data.
        let location = panic
            .location()
            .map(|location| {
                format!(
                    "{}:{}:{}",
                    location.file(),
                    location.line(),
                    location.column()
                )
            })
            .unwrap_or_else(|| "unknown location".into());
        write_record(
            &format!(
                "PANIC on {:?} at {location}; payload omitted for privacy\n{}\n{}",
                std::thread::current().id(),
                Backtrace::force_capture(),
                native_addresses()
            ),
            true,
        );
        if let Some(logger) = LOGGER.get()
            && let Ok(file) = logger.file.try_lock()
        {
            let _ = file.sync_all();
        }
        previous(panic);
    }));
    Some(Session)
}

/// Only call with static labels or non-sensitive diagnostic state, never error text.
pub(crate) fn record(message: &str) {
    write_record(message, false);
}

fn write_record(message: &str, critical: bool) {
    if let Some(logger) = LOGGER.get()
        && let Ok(mut file) = logger.file.try_lock()
        && (critical
            || file
                .metadata()
                .is_ok_and(|metadata| metadata.len() < MAXIMUM_LOG_BYTES))
    {
        let _ = writeln!(file, "{} {message}", chrono::Utc::now().to_rfc3339());
        let _ = file.flush();
    }
}

impl log::Log for SessionLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Warn && metadata.target().starts_with("gpui")
    }

    fn log(&self, entry: &log::Record<'_>) {
        if self.enabled(entry.metadata()) {
            record(&format!(
                "{} {} at {}:{} (message omitted for privacy)",
                entry.level(),
                entry.target(),
                entry.file().unwrap_or("unknown"),
                entry.line().unwrap_or(0)
            ));
        }
    }

    fn flush(&self) {
        if let Ok(mut file) = self.file.try_lock() {
            let _ = file.flush();
        }
    }
}

fn prune_logs(directory: &Path) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut logs = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("session-") && name.ends_with(".log"))
        })
        .collect::<Vec<_>>();
    logs.sort();
    let remove = logs.len().saturating_sub(RETAINED_LOGS - 1);
    for path in logs.into_iter().take(remove) {
        let _ = fs::remove_file(path);
    }
}

#[cfg(target_os = "windows")]
fn native_addresses() -> String {
    use windows_sys::Win32::System::{
        Diagnostics::Debug::RtlCaptureStackBackTrace, LibraryLoader::GetModuleHandleW,
    };
    let mut frames = [std::ptr::null_mut(); 64];
    // Raw addresses remain useful when optimized/stripped binaries cannot resolve
    // private Rust symbols locally. Pair with the matching executable and PDB.
    let (count, base) = unsafe {
        (
            RtlCaptureStackBackTrace(
                0,
                frames.len() as u32,
                frames.as_mut_ptr(),
                std::ptr::null_mut(),
            ),
            GetModuleHandleW(std::ptr::null()),
        )
    };
    let mut output = format!("Native stack addresses; image base={base:p}\n");
    for (index, address) in frames[..count as usize].iter().enumerate() {
        output.push_str(&format!("{index}: {:p}\n", *address));
    }
    output
}

#[cfg(not(target_os = "windows"))]
fn native_addresses() -> String {
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retention_removes_only_old_session_logs() {
        let directory =
            std::env::temp_dir().join(format!("startpoint-log-test-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        for index in 0..12 {
            fs::write(directory.join(format!("session-{index:02}.log")), "test").unwrap();
        }
        fs::write(directory.join("crash.dmp"), "keep").unwrap();
        prune_logs(&directory);
        assert!(!directory.join("session-02.log").exists());
        assert!(directory.join("session-03.log").exists());
        assert!(directory.join("crash.dmp").exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn panic_probe() {
        let Some(directory) = std::env::var_os("STARTPOINT_DIAGNOSTICS_TEST_DIRECTORY") else {
            return;
        };
        let _session = initialize_at(Path::new(&directory)).expect("test log initialization");
        let _ = std::panic::catch_unwind(|| panic!("private-api-key-must-not-be-logged"));
    }

    #[test]
    fn panic_hook_flushes_a_private_backtrace_without_the_payload() {
        let directory =
            std::env::temp_dir().join(format!("startpoint-panic-test-{}", std::process::id()));
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "diagnostics::tests::panic_probe", "--nocapture"])
            .env("STARTPOINT_DIAGNOSTICS_TEST_DIRECTORY", &directory)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "panic diagnostic subprocess failed"
        );
        let path = fs::read_dir(&directory)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let text = fs::read_to_string(path).unwrap();
        assert!(text.contains("PANIC on"));
        assert!(text.contains("payload omitted for privacy"));
        assert!(
            text.contains("diagnostics.rs:"),
            "panic source location is retained"
        );
        assert!(text.contains("0:"), "backtrace contains stack frames");
        #[cfg(target_os = "windows")]
        assert!(text.contains("Native stack addresses; image base="));
        assert!(text.contains("session ended normally"));
        assert!(!text.contains("private-api-key-must-not-be-logged"));
        fs::remove_dir_all(directory).unwrap();
    }
}
