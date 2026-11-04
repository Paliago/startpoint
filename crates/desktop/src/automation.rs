use gpui::App;

pub(crate) fn active(cx: &App) -> bool {
    #[cfg(all(feature = "automation", target_os = "macos"))]
    return cx.try_global::<runtime::Mode>().is_some();
    #[cfg(not(all(feature = "automation", target_os = "macos")))]
    {
        let _ = cx;
        false
    }
}

pub(crate) fn native_allowed(cx: &App, operation: &str) -> bool {
    #[cfg(all(feature = "automation", target_os = "macos"))]
    if let Some(mode) = cx.try_global::<runtime::Mode>() {
        mode.0.deny(operation);
        return false;
    }
    let _ = (cx, operation);
    true
}

#[cfg(all(feature = "automation", target_os = "macos"))]
pub(crate) use runtime::{editor, job_status, run};

#[cfg(all(feature = "automation", target_os = "macos"))]
mod runtime {
    use crate::{components::EditableFieldState, interface::LauncherView};
    use gpui::{prelude::*, *};
    use gpui_automation::{Session, register_input};
    use gpui_component::Root;
    use serde_json::{Value, json};
    use startpoint_sdk::{
        ApiError, AppEvent, Application, ApplicationApi, ApplicationPaths, Command, JobId,
        JobStatus, Snapshot,
    };
    use std::{
        collections::VecDeque,
        path::PathBuf,
        sync::{Arc, Mutex},
    };

    pub(crate) struct Policy {
        endpoint: PathBuf,
        metadata: bool,
        denied: Mutex<VecDeque<String>>,
        receipts: Mutex<VecDeque<Value>>,
    }
    impl Policy {
        fn permits(&self, command: &Command) -> bool {
            self.metadata && matches!(command, Command::UpdateMetadata { .. })
        }
        pub fn deny(&self, operation: &str) {
            let mut denied = self
                .denied
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            denied.push_back(operation.into());
            if denied.len() > 64 {
                denied.pop_front();
            }
        }
        fn status(&self) -> Value {
            json!({
                "kind": "sqlite", "endpoint": self.endpoint,
                "metadataWritesAllowed": self.metadata,
                "otherWritesAllowed": false, "systemCredentials": false, "nativeActions": false,
                "denied": self.denied.lock().unwrap_or_else(|error| error.into_inner()).iter().cloned().collect::<Vec<_>>(),
                "jobHistory": self.receipts.lock().unwrap_or_else(|error| error.into_inner()).iter().cloned().collect::<Vec<_>>()
            })
        }
    }
    pub(crate) struct Mode(pub Arc<Policy>);
    impl Global for Mode {}

    struct RestrictedApplication {
        inner: Application,
        policy: Arc<Policy>,
    }
    impl ApplicationApi for RestrictedApplication {
        fn snapshot(&self) -> Snapshot {
            self.inner.snapshot()
        }
        fn subscribe(&self) -> async_channel::Receiver<AppEvent> {
            self.inner.subscribe()
        }
        fn cancel(&self, job: JobId) {
            self.inner.cancel(job);
        }
        fn submit(&self, command: Command) -> Result<JobId, ApiError> {
            if self.policy.permits(&command) {
                return self.inner.submit(command);
            }
            self.policy.deny(command.label());
            Err(ApiError::Validation(format!(
                "Automation policy denied: {}",
                command.label()
            )))
        }
    }

    pub(crate) fn job_status(job: &JobStatus) -> Value {
        json!({"id": job.id, "label": job.label, "state": format!("{:?}", job.state), "error": job.error})
    }

    pub(crate) fn run() -> Result<(), String> {
        if cfg!(feature = "self-update") {
            return Err("Automation must not be combined with self-update".into());
        }
        let mut socket = None;
        let mut library = None;
        let mut metadata = false;
        let mut arguments = std::env::args().skip(1);
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--automation-socket" => {
                    socket = Some(PathBuf::from(
                        arguments.next().ok_or("Missing socket path")?,
                    ))
                }
                "--automation-library" => {
                    library = Some(PathBuf::from(
                        arguments.next().ok_or("Missing library path")?,
                    ))
                }
                "--automation-allow-metadata" => metadata = true,
                _ => return Err(format!("Unsupported automation argument: {argument}")),
            }
        }
        let session = Session::new(
            socket.ok_or("Provide --automation-socket /private/directory/application.sock")?,
        )?;
        let directory = session
            .directory()
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let paths = ApplicationPaths {
            data_directory: directory.join("data"),
            cache_directory: directory.join("cache"),
        };
        let policy = Arc::new(Policy {
            endpoint: paths.data_directory.join("library.sqlite3"),
            metadata,
            denied: Mutex::default(),
            receipts: Mutex::default(),
        });
        println!(
            "{}",
            json!({"automationBackend": policy.status(), "sessionDirectory": directory})
        );
        let _diagnostics = crate::diagnostics::initialize_at(&directory.join("diagnostics"));
        if let Some(source) = library {
            let source = source.canonicalize().map_err(|error| error.to_string())?;
            let count = startpoint_core::seed_automation_library(&source, &paths)
                .map_err(|error| error.to_string())?;
            println!("{}", json!({"readOnlySeed": source, "games": count}));
        }
        let inner = startpoint_core::LauncherBuilder::new()
            .without_system_credentials()
            .discovery_provider(startpoint_core::SteamProvider)
            .discovery_provider(startpoint_core::EpicProvider)
            .discovery_provider(startpoint_core::GogProvider)
            .artwork_provider(
                startpoint_core::SteamGridDbProvider::new().map_err(|error| error.to_string())?,
            )
            .build(paths)
            .map_err(|error| error.to_string())?;
        let events = inner.subscribe();
        let observations = policy.clone();
        std::thread::Builder::new()
            .name("automation-receipts".into())
            .spawn(move || {
                while let Ok(event) = events.recv_blocking() {
                    if let AppEvent::JobChanged(job) = event {
                        let mut history = observations
                            .receipts
                            .lock()
                            .unwrap_or_else(|error| error.into_inner());
                        history.push_back(job_status(&job));
                        if history.len() > 64 {
                            history.pop_front();
                        }
                    }
                }
            })
            .map_err(|error| error.to_string())?;
        let api: Application = Arc::new(RestrictedApplication {
            inner,
            policy: policy.clone(),
        });
        let startup_error = Arc::new(Mutex::new(None));
        let window_error = startup_error.clone();
        gpui_platform::application()
            .with_assets(crate::LauncherAssets)
            .run(move |cx| {
                gpui_automation::prepare_application();
                cx.set_global(Mode(policy.clone()));
                crate::initialize_interface(&api, cx);
                let mut options = gpui_automation::window_options();
                options.window_min_size = Some(size(px(960.), px(620.)));
                options.titlebar = Some(TitlebarOptions {
                    title: Some("Startpoint — Automation".into()),
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(18.), px(14.))),
                });
                options.window_background = crate::appearance::background(cx);
                let result = cx
                    .open_window(options, |window, cx| {
                        crate::appearance::configure_window(window);
                        let view = cx.new(|cx| LauncherView::new(api.clone(), window, cx));
                        cx.new(|cx| Root::new(view, window, cx).bg(transparent_black()))
                    })
                    .map_err(|error| error.to_string())
                    .and_then(|window| {
                        session.start(window, cx, move |root, _, cx| {
                            let mut status = root
                                .view()
                                .clone()
                                .downcast::<LauncherView>()
                                .map(|view| view.read(cx).automation_status(cx))
                                .unwrap_or_else(|_| json!({"error": "Launcher root unavailable"}));
                            status["backend"] = policy.status();
                            status
                        })
                    });
                if let Err(error) = result {
                    eprintln!("Automation window failed: {error}");
                    *window_error
                        .lock()
                        .unwrap_or_else(|error| error.into_inner()) = Some(error);
                    cx.quit();
                }
            });
        let error = startup_error
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        error.map_or(Ok(()), Err)
    }

    pub(crate) fn editor<S: EditableFieldState + Focusable>(
        id: &str,
        label: &str,
        state: &Entity<S>,
        disabled: bool,
        element: AnyElement,
        cx: &App,
    ) -> AnyElement {
        if !super::active(cx) {
            return element;
        }
        let focus = state.read(cx).focus_handle(cx);
        let value = state.read(cx).automation_snapshot().value;
        let state = state.downgrade();
        let id = id.to_string();
        let label = format!("{label} editor");
        div()
            .id(SharedString::from(id.clone()))
            .role(Role::TextInput)
            .aria_label(label.clone())
            .aria_value(value)
            .track_focus(&focus)
            .relative()
            .w_full()
            .child(element)
            .child(
                canvas(
                    move |bounds, window, cx| {
                        let state = state.clone();
                        register_input(
                            id.clone(),
                            label.clone(),
                            bounds,
                            focus.clone(),
                            move |cx| {
                                let state = state.upgrade()?;
                                let mut snapshot = state.read(cx).automation_snapshot();
                                snapshot.enabled &= !disabled;
                                Some(snapshot)
                            },
                            window,
                            cx,
                        );
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0()
                .size_full(),
            )
            .into_any_element()
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[::core::prelude::v1::test]
        fn write_policy_denies_everything_except_explicitly_authorized_metadata() {
            let mut policy = Policy {
                endpoint: PathBuf::from("/private/session/library.sqlite3"),
                metadata: false,
                denied: Mutex::default(),
                receipts: Mutex::default(),
            };
            let id = startpoint_sdk::GameId("example:42".into());
            let metadata = Command::UpdateMetadata {
                game_id: id.clone(),
                metadata: startpoint_sdk::UserMetadata::default(),
            };
            assert!(!policy.permits(&metadata));
            policy.metadata = true;
            assert!(policy.permits(&metadata));
            for command in [
                Command::LaunchGame { game_id: id },
                Command::RefreshLibrary,
                Command::RefreshMetadata,
                Command::SetAppearance {
                    settings: Default::default(),
                },
                Command::SetArtworkCredential {
                    provider_id: "steamgriddb".into(),
                    credential: Some("not-a-real-key".into()),
                },
            ] {
                assert!(!policy.permits(&command));
            }
        }
    }
}
