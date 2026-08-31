//! Windows login startup registration for the current TRACE executable.

use serde::Serialize;

#[cfg(windows)]
const REGISTRY_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(windows)]
const REGISTRY_VALUE: &str = "TRACE";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StartupSettings {
    supported: bool,
    enabled: bool,
}

#[tauri::command]
pub(crate) fn startup_settings() -> Result<StartupSettings, String> {
    platform_startup_settings()
}

#[tauri::command]
pub(crate) fn set_launch_on_startup(enabled: bool) -> Result<StartupSettings, String> {
    platform_set_launch_on_startup(enabled)
}

#[cfg(windows)]
fn platform_startup_settings() -> Result<StartupSettings, String> {
    let current_executable = std::env::current_exe()
        .map_err(|error| format!("could not locate the TRACE executable: {error}"))?;
    let registered = query_registered_command()?;
    Ok(StartupSettings {
        supported: true,
        enabled: registered.is_some_and(|command| {
            normalise_registered_command(&command)
                .eq_ignore_ascii_case(current_executable.to_string_lossy().as_ref())
        }),
    })
}

#[cfg(not(windows))]
#[allow(clippy::unnecessary_wraps)] // The command keeps one Result-shaped IPC contract on every target.
fn platform_startup_settings() -> Result<StartupSettings, String> {
    Ok(StartupSettings {
        supported: false,
        enabled: false,
    })
}

#[cfg(windows)]
fn platform_set_launch_on_startup(enabled: bool) -> Result<StartupSettings, String> {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let current_executable = std::env::current_exe()
        .map_err(|error| format!("could not locate the TRACE executable: {error}"))?;
    let mut command = std::process::Command::new("reg.exe");
    command.creation_flags(CREATE_NO_WINDOW);
    if enabled {
        let registered_command = format!("\"{}\"", current_executable.display());
        command.args([
            "ADD",
            REGISTRY_KEY,
            "/v",
            REGISTRY_VALUE,
            "/t",
            "REG_SZ",
            "/d",
            &registered_command,
            "/f",
        ]);
    } else {
        if query_registered_command()?.is_none() {
            return platform_startup_settings();
        }
        command.args(["DELETE", REGISTRY_KEY, "/v", REGISTRY_VALUE, "/f"]);
    }
    let output = command
        .output()
        .map_err(|error| format!("could not update Windows startup settings: {error}"))?;
    if !output.status.success() {
        return Err(registry_error(&output));
    }
    platform_startup_settings()
}

#[cfg(not(windows))]
fn platform_set_launch_on_startup(_enabled: bool) -> Result<StartupSettings, String> {
    Err("launching at login is currently available only on Windows".into())
}

#[cfg(windows)]
fn query_registered_command() -> Result<Option<String>, String> {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let output = std::process::Command::new("reg.exe")
        .args(["QUERY", REGISTRY_KEY, "/v", REGISTRY_VALUE])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| format!("could not read Windows startup settings: {error}"))?;
    if output.status.success() {
        return Ok(parse_registered_command(&String::from_utf8_lossy(
            &output.stdout,
        )));
    }
    if output.status.code() == Some(1) {
        return Ok(None);
    }
    Err(registry_error(&output))
}

#[cfg(any(windows, test))]
fn parse_registered_command(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let (_, value) = line.split_once("REG_SZ")?;
        let value = value.trim();
        (!value.is_empty()).then_some(value.to_owned())
    })
}

#[cfg(any(windows, test))]
fn normalise_registered_command(command: &str) -> &str {
    let command = command.trim();
    command
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(command)
}

#[cfg(windows)]
fn registry_error(output: &std::process::Output) -> String {
    let error = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if error.is_empty() {
        "Windows rejected the startup setting change".into()
    } else {
        format!("Windows rejected the startup setting change: {error}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_registry_run_value_without_splitting_spaced_paths() {
        let output = r#"
HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run
    TRACE    REG_SZ    "C:\Program Files\TRACE\trace.exe"
"#;
        assert_eq!(
            parse_registered_command(output).as_deref(),
            Some(r#""C:\Program Files\TRACE\trace.exe""#)
        );
    }

    #[test]
    fn normalises_only_the_outer_executable_quotes() {
        assert_eq!(
            normalise_registered_command(r#""C:\Program Files\TRACE\trace.exe""#),
            r"C:\Program Files\TRACE\trace.exe"
        );
        assert_eq!(
            normalise_registered_command(r"C:\TRACE\trace.exe"),
            r"C:\TRACE\trace.exe"
        );
    }
}
