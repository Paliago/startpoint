use startpoint_sdk::{ApiError, ApplicationPaths, ExecutableLaunch, LaunchTarget};
use std::{
    path::Path,
    process::{Command, Stdio},
};

pub fn application_paths() -> Result<ApplicationPaths, ApiError> {
    let directories = directories::ProjectDirs::from("", "", "Startpoint").ok_or_else(|| {
        ApiError::Io("Could not locate this user's application directories".into())
    })?;
    Ok(ApplicationPaths {
        data_directory: directories.data_local_dir().to_path_buf(),
        cache_directory: directories.cache_dir().to_path_buf(),
    })
}

pub(crate) fn validate_launch(launch: &ExecutableLaunch) -> Result<(), ApiError> {
    if !launch.executable.is_absolute() || !launch.executable.exists() {
        return Err(ApiError::Validation(
            "Choose an existing executable or application bundle using its absolute path".into(),
        ));
    }
    let bundle = launch
        .executable
        .extension()
        .is_some_and(|extension| extension == "app");
    if bundle && launch.working_directory.is_some() {
        return Err(ApiError::Validation("LaunchServices does not support a custom working directory. Choose the executable in Contents/MacOS instead of the .app bundle.".into()));
    }
    if !launch.executable.is_file()
        && !(cfg!(target_os = "macos") && bundle && launch.executable.is_dir())
    {
        return Err(ApiError::Validation(
            "The launch target is not an executable file or macOS application bundle".into(),
        ));
    }
    if launch
        .arguments
        .iter()
        .any(|argument| argument.contains('\0'))
    {
        return Err(ApiError::Validation(
            "Launch arguments cannot contain NUL characters".into(),
        ));
    }
    if let Some(directory) = &launch.working_directory
        && (!directory.is_absolute() || !directory.is_dir())
    {
        return Err(ApiError::Validation(
            "The working directory must be an existing absolute directory".into(),
        ));
    }
    Ok(())
}

pub(crate) fn launch(target: LaunchTarget, allowed_schemes: &[String]) -> Result<(), ApiError> {
    match target {
        LaunchTarget::Uri(uri) => {
            let parsed = reqwest::Url::parse(&uri)
                .map_err(|_| ApiError::Validation("Invalid launcher URI".into()))?;
            if !allowed_schemes
                .iter()
                .any(|scheme| scheme == parsed.scheme())
            {
                return Err(ApiError::Validation(
                    "This launcher URI scheme is not supported".into(),
                ));
            }
            open::that(uri).map_err(|error| ApiError::Io(error.to_string()))?;
        }
        LaunchTarget::Executable(configuration) => {
            validate_launch(&configuration)?;
            let mut command = Command::new(&configuration.executable);
            command.args(&configuration.arguments);
            if let Some(directory) = configuration
                .working_directory
                .as_deref()
                .or_else(|| configuration.executable.parent())
            {
                command.current_dir(directory);
            }
            detach(command)?;
        }
        LaunchTarget::ApplicationBundle(configuration) => {
            validate_launch(&configuration)?;
            if !cfg!(target_os = "macos") {
                return Err(ApiError::Validation(
                    "Application bundles can only be launched on macOS".into(),
                ));
            }
            let mut command = Command::new(Path::new("/usr/bin/open"));
            command.arg("-a").arg(&configuration.executable);
            if !configuration.arguments.is_empty() {
                command.arg("--args").args(&configuration.arguments);
            }
            if let Some(directory) = &configuration.working_directory {
                command.current_dir(directory);
            }
            detach(command)?;
        }
    }
    Ok(())
}

fn detach(mut command: Command) -> Result<(), ApiError> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| ApiError::Io(error.to_string()))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

pub(crate) fn read_api_key(provider_id: &str) -> Result<Option<String>, ApiError> {
    let entry = keyring::Entry::new("Startpoint", provider_id).map_err(credential_error)?;
    match entry.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(credential_error(error)),
    }
}

pub(crate) fn write_api_key(provider_id: &str, key: Option<&str>) -> Result<(), ApiError> {
    let entry = keyring::Entry::new("Startpoint", provider_id).map_err(credential_error)?;
    match key {
        Some(key) => entry.set_password(key).map_err(credential_error),
        None => match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(credential_error(error)),
        },
    }
}

fn credential_error(error: keyring::Error) -> ApiError {
    ApiError::Provider(format!(
        "The OS credential store could not be accessed: {error}. The key has not been written to a file."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_arguments_round_trip_without_shell_interpretation() {
        let arguments = vec![
            "C:\\Program Files\\Game\\save files".to_string(),
            "two words".to_string(),
            "".to_string(),
            "--title=世界".to_string(),
        ];
        let launch = ExecutableLaunch {
            executable: std::env::current_exe().expect("test executable"),
            arguments: arguments.clone(),
            working_directory: None,
        };
        assert!(validate_launch(&launch).is_ok());
        let stored = serde_json::to_string(&launch).expect("serialize launch");
        let loaded: ExecutableLaunch = serde_json::from_str(&stored).expect("deserialize launch");
        assert_eq!(loaded.arguments, arguments);
        let invalid = ExecutableLaunch {
            arguments: vec!["invalid\0argument".into()],
            ..loaded
        };
        assert!(matches!(
            validate_launch(&invalid),
            Err(ApiError::Validation(_))
        ));
    }
}
