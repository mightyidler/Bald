use std::{
    os::windows::process::CommandExt,
    process::{Command, Stdio},
};

use anyhow::{Context, Result};
use windows::{
    Win32::Foundation::ERROR_FILE_NOT_FOUND,
    Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegDeleteValueW,
        RegOpenKeyExW, RegSetValueExW,
    },
    core::PCWSTR,
};

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const VALUE_NAME: &str = "Bald";
const ELEVATED_TASK_NAME: &str = "Bald Elevated";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn hidden_command(program: &str) -> Command {
    let mut command = Command::new(program);
    command
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    command
}

pub const fn is_development_build() -> bool {
    cfg!(debug_assertions)
}

pub fn should_launch_elevated_task(arguments: &[String], elevated: bool, replace: bool) -> bool {
    !elevated
        && !replace
        && arguments
            .iter()
            .any(|arg| matches!(arg.as_str(), "--autostart" | "--background"))
}

pub fn remove_elevated_task() -> Result<()> {
    let executable = std::env::current_exe().context("resolve Bald executable")?;
    let script = format!(
        "$ErrorActionPreference='Stop'; {}; if ($owned) {{ Unregister-ScheduledTask -InputObject $t -Confirm:$false }}",
        owned_task_query(&executable.to_string_lossy())
    );
    let status = hidden_command("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &script,
        ])
        .stdout(Stdio::null())
        .status()
        .context("remove owned elevated Bald task")?;
    if !status.success() {
        anyhow::bail!("remove elevated Bald task failed: {status}");
    }
    Ok(())
}

pub fn launch_elevated_task() -> Result<bool> {
    if is_development_build() {
        return Ok(false);
    }

    let executable = std::env::current_exe().context("resolve Bald executable")?;
    let script = format!(
        "$ErrorActionPreference='Stop'; {}; if ($owned) {{ Start-ScheduledTask -InputObject $t; exit 0 }}; exit 1",
        owned_task_query(&executable.to_string_lossy())
    );
    Ok(hidden_command("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &script,
        ])
        .stdout(Stdio::null())
        .status()
        .context("launch elevated Bald task")?
        .success())
}

fn owned_task_query(executable: &str) -> String {
    let executable = executable.replace('\'', "''");
    format!(
        "$t=Get-ScheduledTask -TaskName '{ELEVATED_TASK_NAME}' -TaskPath '\\' -ErrorAction SilentlyContinue; \
        $owned=$t -and @($t.Actions).Count -eq 1 -and $t.Actions[0].Execute -eq '{executable}' \
        -and $t.Actions[0].Arguments -eq '--elevated-task --background'"
    )
}

pub fn ensure_elevated_task() -> Result<()> {
    if is_development_build() {
        return Ok(());
    }

    use base64::{Engine, engine::general_purpose::STANDARD};

    let executable = std::env::current_exe().context("resolve Bald executable")?;
    let ownership = owned_task_query(&executable.to_string_lossy());
    let executable = executable.to_string_lossy().replace('\'', "''");
    let script = format!(
        "$ErrorActionPreference='Stop'; {ownership}; if ($t -and !$owned) {{ throw 'Elevated task belongs to another executable' }}; \
         $a=New-ScheduledTaskAction -Execute '{executable}' -Argument '--elevated-task --background';\
         $p=New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType Interactive -RunLevel Highest;\
         $s=New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero);\
         Register-ScheduledTask -TaskName '{ELEVATED_TASK_NAME}' -Action $a -Principal $p -Settings $s -Force | Out-Null"
    );
    let encoded = STANDARD.encode(
        script
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    let status = hidden_command("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-EncodedCommand",
            &encoded,
        ])
        .stdout(Stdio::null())
        .status()
        .context("register elevated Bald task")?;
    if !status.success() {
        anyhow::bail!("register elevated Bald task failed: {status}");
    }
    Ok(())
}

pub fn set_enabled(enabled: bool) -> Result<()> {
    if is_development_build() {
        return Ok(());
    }

    let command = if enabled {
        let exe = std::env::current_exe().context("resolve Bald executable")?;
        Some(wide(&format!("\"{}\" --autostart", exe.display())))
    } else {
        None
    };
    unsafe {
        let mut key = HKEY::default();
        let run_key = wide(RUN_KEY);
        let opened = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(run_key.as_ptr()),
            None,
            KEY_SET_VALUE,
            &mut key,
        );
        if !enabled && opened == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        opened.ok().context("open per-user Run registry key")?;
        let name = wide(VALUE_NAME);
        let result = if let Some(command) = command {
            let bytes =
                std::slice::from_raw_parts(command.as_ptr() as *const u8, command.len() * 2);
            RegSetValueExW(key, PCWSTR(name.as_ptr()), None, REG_SZ, Some(bytes))
        } else {
            RegDeleteValueW(key, PCWSTR(name.as_ptr()))
        };
        let _ = RegCloseKey(key);
        if !enabled && result == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        result.ok().context("update per-user startup registration")
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn task_ownership_requires_one_exact_executable_action() {
        use base64::{Engine, engine::general_purpose::STANDARD};
        let path = "C:\\테스트 O'Brien\\Bald.exe";
        let quoted = path.replace('\'', "''");
        for (actions, expected) in [
            (
                format!(
                    "@([pscustomobject]@{{Execute='{quoted}';Arguments='--elevated-task --background'}})"
                ),
                "True",
            ),
            (
                format!(
                    "@([pscustomobject]@{{Execute='{quoted}.other';Arguments='--elevated-task --background'}})"
                ),
                "False",
            ),
            (
                format!(
                    "@([pscustomobject]@{{Execute='{quoted}';Arguments='--elevated-task --background'}},[pscustomobject]@{{Execute='{quoted}';Arguments='--elevated-task --background'}})"
                ),
                "False",
            ),
            ("@()".into(), "False"),
            (
                format!("@([pscustomobject]@{{Execute='{quoted}';Arguments='--other'}})"),
                "False",
            ),
        ] {
            // Mock the query; never read or modify the user's scheduled tasks.
            let script = format!(
                "function Get-ScheduledTask {{ param($TaskName,$TaskPath,$ErrorAction); if($TaskPath -ne '\\') {{throw 'wrong task folder'}}; [pscustomobject]@{{ Actions={actions} }} }}; {}; [string][bool]$owned",
                owned_task_query(path)
            );
            let encoded = STANDARD.encode(
                script
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>(),
            );
            let output = hidden_command("powershell.exe")
                .args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded])
                .output()
                .unwrap();
            assert!(output.status.success(), "{output:?}");
            assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), expected);
        }
    }
    #[test]
    fn manual_open_never_launches_background_task() {
        assert!(!should_launch_elevated_task(
            &["bald.exe".into()],
            false,
            false
        ));
        for flag in ["--autostart", "--background"] {
            let args = vec!["bald.exe".into(), flag.into()];
            assert!(should_launch_elevated_task(&args, false, false));
            assert!(!should_launch_elevated_task(&args, true, false));
            assert!(!should_launch_elevated_task(&args, false, true));
        }
    }
}
