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
