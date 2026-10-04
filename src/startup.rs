use std::{
    os::windows::process::CommandExt,
    process::{Command, Stdio},
};

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

pub fn launch_elevated_task() -> Result<bool> {
    if is_development_build() {
        return Ok(false);
    }

    let query = hidden_command("schtasks.exe")
        .args(["/Query", "/TN", ELEVATED_TASK_NAME, "/XML"])
        .output()
        .context("query elevated Bald task")?;
    if !query.status.success() {
        return Ok(false);
    }

    let executable = std::env::current_exe().context("resolve Bald executable")?;
    let task_xml = String::from_utf8_lossy(&query.stdout).to_lowercase();
    let executable = executable.to_string_lossy().to_lowercase();
    if !task_xml.contains(&executable) {
        return Ok(false);
    }

    Ok(hidden_command("schtasks.exe")
        .args(["/Run", "/TN", ELEVATED_TASK_NAME])
        .stdout(Stdio::null())
        .status()
        .context("launch elevated Bald task")?
        .success())
}

pub fn ensure_elevated_task() -> Result<()> {
    if is_development_build() {
        return Ok(());
    }

    use base64::{Engine, engine::general_purpose::STANDARD};

    let executable = std::env::current_exe().context("resolve Bald executable")?;
    let executable = executable.to_string_lossy().replace('\'', "''");
    let script = format!(
        "$a=New-ScheduledTaskAction -Execute '{executable}' -Argument '--elevated-task --background';\
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
