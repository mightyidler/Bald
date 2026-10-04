use std::process::{Command, Stdio};

use anyhow::{Context, Result};
use windows::{
    Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegDeleteValueW,
        RegOpenKeyExW, RegSetValueExW,
    },
    core::PCWSTR,
};

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const VALUE_NAME: &str = "Bald";
const ELEVATED_TASK_NAME: &str = "Bald Elevated";

pub fn launch_elevated_task() -> Result<bool> {
    let exists = Command::new("schtasks.exe")
        .args(["/Query", "/TN", ELEVATED_TASK_NAME])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("query elevated Bald task")?
        .success();
    if !exists {
        return Ok(false);
    }
    Ok(Command::new("schtasks.exe")
        .args(["/Run", "/TN", ELEVATED_TASK_NAME])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("launch elevated Bald task")?
        .success())
}

pub fn ensure_elevated_task() -> Result<()> {
    use base64::{Engine, engine::general_purpose::STANDARD};

    let executable = std::env::current_exe().context("resolve Bald executable")?;
    let executable = executable.to_string_lossy().replace('\'', "''");
    let script = format!(
        "$a=New-ScheduledTaskAction -Execute '{executable}' -Argument '--elevated-task';\
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
    let status = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-EncodedCommand",
            &encoded,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("register elevated Bald task")?;
    if !status.success() {
        anyhow::bail!("register elevated Bald task failed: {status}");
    }
    Ok(())
}

pub fn set_enabled(enabled: bool) -> Result<()> {
    unsafe {
        let mut key = HKEY::default();
        let run_key = wide(RUN_KEY);
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(run_key.as_ptr()),
            None,
            KEY_SET_VALUE,
            &mut key,
        )
        .ok()
        .context("open per-user Run registry key")?;
        let name = wide(VALUE_NAME);
        let result = if enabled {
            let exe = std::env::current_exe().context("resolve Bald executable")?;
            let command = wide(&format!("\"{}\" --autostart", exe.display()));
            let bytes =
                std::slice::from_raw_parts(command.as_ptr() as *const u8, command.len() * 2);
            RegSetValueExW(key, PCWSTR(name.as_ptr()), None, REG_SZ, Some(bytes))
        } else {
            RegDeleteValueW(key, PCWSTR(name.as_ptr()))
        };
        let _ = RegCloseKey(key);
        result.ok().context("update per-user startup registration")
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
