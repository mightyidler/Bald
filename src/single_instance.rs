use anyhow::{Context, Result};
use windows::{
    Win32::{
        Foundation::{
            CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0,
        },
        System::Threading::{CreateMutexW, INFINITE, ReleaseMutex, WaitForSingleObject},
        UI::WindowsAndMessaging::{
            FindWindowW, PostMessageW, SW_RESTORE, SetForegroundWindow, ShowWindow,
        },
    },
    core::PCWSTR,
};

pub struct SingleInstance {
    handle: HANDLE,
    primary: bool,
}

impl SingleInstance {
    pub fn acquire() -> Result<Self> {
        Self::acquire_named("Local\\Bald.SingleInstance.v1")
    }

    fn acquire_named(name: &str) -> Result<Self> {
        let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
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

    pub fn wait_for_primary(&mut self) -> Result<()> {
        if !self.primary {
            let result = unsafe { WaitForSingleObject(self.handle, INFINITE) };
            if result != WAIT_OBJECT_0 && result != WAIT_ABANDONED {
                anyhow::bail!("wait for previous BALD instance failed: {}", result.0);
            }
            self.primary = true;
        }
        Ok(())
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe {
            if self.primary {
                let _ = ReleaseMutex(self.handle);
            }
            let _ = CloseHandle(self.handle);
        }
    }
}

pub fn show_existing() {
    let title: Vec<u16> = "Bald\0".encode_utf16().collect();
    unsafe {
        let activation: Vec<u16> = crate::ui_activation::TITLE
            .encode_utf16()
            .chain(Some(0))
            .collect();
        if let Ok(hwnd) = FindWindowW(None, PCWSTR(activation.as_ptr())) {
            let _ = PostMessageW(
                Some(hwnd),
                crate::ui_activation::OPEN_MESSAGE,
                Default::default(),
                Default::default(),
            );
            return;
        }
        if let Ok(hwnd) = FindWindowW(None, PCWSTR(title.as_ptr())) {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replacement_waits_for_owner_even_when_shutdown_takes_over_five_seconds() {
        let name = format!("Local\\Bald.TestReplacement.{}", uuid::Uuid::new_v4());
        let owner = SingleInstance::acquire_named(&name).unwrap();
        assert!(owner.is_primary());
        let (ready, received) = std::sync::mpsc::channel();
        let replacement = std::thread::spawn(move || {
            let mut instance = SingleInstance::acquire_named(&name).unwrap();
            assert!(!instance.is_primary());
            ready.send(()).unwrap();
            instance.wait_for_primary().unwrap();
            assert!(instance.is_primary());
        });
        received.recv().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5100));
        assert!(!replacement.is_finished());
        drop(owner);
        replacement.join().unwrap();
    }
}
