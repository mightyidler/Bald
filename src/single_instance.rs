use anyhow::{Context, Result};
use windows::{
    Win32::{
        Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE},
        System::Threading::CreateMutexW,
        UI::WindowsAndMessaging::{FindWindowW, SW_RESTORE, SetForegroundWindow, ShowWindow},
    },
    core::PCWSTR,
};

pub struct SingleInstance {
    handle: HANDLE,
    primary: bool,
}

impl SingleInstance {
    pub fn acquire() -> Result<Self> {
        let name: Vec<u16> = "Local\\Bald.SingleInstance.v1\0".encode_utf16().collect();
        unsafe {
            let handle =
                CreateMutexW(None, true, PCWSTR(name.as_ptr())).context("create instance mutex")?;
            let primary = GetLastError() != ERROR_ALREADY_EXISTS;
            Ok(Self { handle, primary })
        }
    }

    pub fn is_primary(&self) -> bool {
        self.primary
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

pub fn show_existing() {
    let title: Vec<u16> = "Bald\0".encode_utf16().collect();
    unsafe {
        if let Ok(hwnd) = FindWindowW(None, PCWSTR(title.as_ptr())) {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(hwnd);
        }
    }
}
