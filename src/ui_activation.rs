//! A native window receives second-instance requests while the WebView is absent.
use anyhow::{Result, bail};
use std::sync::{Arc, Mutex, Weak};
use tauri::AppHandle;
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{
            ChangeWindowMessageFilterEx, CreateWindowExW, DefWindowProcW, DestroyWindow,
            MSGFLT_ALLOW, RegisterClassW, WINDOW_STYLE, WM_APP, WM_NCDESTROY, WNDCLASSW,
            WS_EX_TOOLWINDOW,
        },
    },
};

pub const TITLE: &str = "Bald.Activation.v1";
pub const OPEN_MESSAGE: u32 = WM_APP + 19;
const SUBCLASS_ID: usize = 0x42414c45;
type Application = Weak<Mutex<Option<AppHandle>>>;
pub struct ActivationWindow(isize);

impl Drop for ActivationWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(HWND(self.0 as *mut _));
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

pub fn create(app: &Arc<Mutex<Option<AppHandle>>>) -> Result<ActivationWindow> {
    let title: Vec<u16> = TITLE.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let module = GetModuleHandleW(None)?;
        let class = windows::core::w!("Bald.ActivationWindow.v1");
        if RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: windows::Win32::Foundation::HINSTANCE(module.0),
            lpszClassName: class,
            ..Default::default()
        }) == 0
        {
            bail!("register activation window failed");
        }
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class,
            windows::core::PCWSTR(title.as_ptr()),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            None,
            None,
            Some(windows::Win32::Foundation::HINSTANCE(module.0)),
            None,
        )?;
        let window = ActivationWindow(hwnd.0 as isize);
        // A non-elevated second instance can request opening an elevated UI.
        ChangeWindowMessageFilterEx(hwnd, OPEN_MESSAGE, MSGFLT_ALLOW, None)?;
        install(hwnd, app)?;
        Ok(window)
    }
}

fn install(hwnd: HWND, app: &Arc<Mutex<Option<AppHandle>>>) -> Result<()> {
    let data = Box::into_raw(Box::new(Arc::downgrade(app)));
    if !unsafe { SetWindowSubclass(hwnd, Some(activation_proc), SUBCLASS_ID, data as usize) }
        .as_bool()
    {
        unsafe {
            drop(Box::from_raw(data));
        }
        bail!("install Bald activation handler failed");
    }
    Ok(())
}

unsafe extern "system" fn activation_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    if message == OPEN_MESSAGE {
        let application = unsafe { &*(data as *const Application) };
        if let Some(owner) = application.upgrade()
            && let Some(app) = owner.lock().unwrap().clone()
        {
            crate::web_app::open_main_window(&app);
        }
        return LRESULT(0);
    }
    unsafe {
        if message == WM_NCDESTROY {
            let _ = RemoveWindowSubclass(hwnd, Some(activation_proc), SUBCLASS_ID);
            drop(Box::from_raw(data as *mut Application));
        }
        DefSubclassProc(hwnd, message, wparam, lparam)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::{IsWindow, IsWindowVisible, SendMessageW};

    #[test]
    fn activation_window_is_hidden_and_does_not_keep_the_application_alive() {
        let application = Arc::new(Mutex::new(None));
        let window = create(&application).unwrap();
        let hwnd = HWND(window.0 as *mut _);
        assert_eq!(Arc::strong_count(&application), 1);
        assert!(!unsafe { IsWindowVisible(hwnd) }.as_bool());
        drop(application);
        unsafe {
            SendMessageW(hwnd, OPEN_MESSAGE, None, None);
        }
        drop(window);
        assert!(!unsafe { IsWindow(Some(hwnd)) }.as_bool());
    }
}
