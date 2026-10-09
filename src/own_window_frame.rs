//! Non-client handling for Bald's own UI only. Never attach to another process.
use anyhow::{Result, bail};
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    System::Threading::{GetCurrentProcessId, GetCurrentThreadId},
    UI::{
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{
            GetWindowThreadProcessId, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
            SWP_NOZORDER, SetWindowPos, WM_NCCALCSIZE, WM_NCDESTROY,
        },
    },
};

const SUBCLASS_ID: usize = 0x42414c44;

pub fn install(hwnd: HWND) -> Result<()> {
    unsafe {
        let mut process_id = 0;
        let thread_id = GetWindowThreadProcessId(hwnd, Some(&mut process_id));
        if process_id != GetCurrentProcessId() || thread_id != GetCurrentThreadId() {
            bail!("Bald frame handler must run on its own window thread");
        }
        if !SetWindowSubclass(hwnd, Some(frame_proc), SUBCLASS_ID, 0).as_bool() {
            bail!("install Bald frame handler failed");
        }
        SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        )?;
    }
    Ok(())
}

unsafe extern "system" fn frame_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    // Tao already handles TRUE. FALSE must also leave the proposed rectangle
    // untouched; DefWindowProc otherwise calculates the retained WS_CAPTION.
    if message == WM_NCCALCSIZE {
        return LRESULT(0);
    }
    unsafe {
        if message == WM_NCDESTROY {
            let _ = RemoveWindowSubclass(hwnd, Some(frame_proc), SUBCLASS_ID);
        }
        DefSubclassProc(hwnd, message, wparam, lparam)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    unsafe extern "system" fn fixture_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        unsafe {
            windows::Win32::UI::WindowsAndMessaging::DefWindowProcW(hwnd, message, wparam, lparam)
        }
    }
    use windows::{
        Win32::{
            Foundation::HINSTANCE,
            Foundation::RECT,
            System::LibraryLoader::GetModuleHandleW,
            UI::WindowsAndMessaging::{
                CreateWindowExW, DestroyWindow, GetClientRect, GetWindowRect, NCCALCSIZE_PARAMS,
                RegisterClassW, SendMessageW, WNDCLASSW, WS_OVERLAPPEDWINDOW,
            },
        },
        core::w,
    };

    #[test]
    fn own_ui_preserves_full_client_for_both_nonclient_calculation_forms() {
        unsafe {
            let module = GetModuleHandleW(None).unwrap();
            assert_ne!(
                RegisterClassW(&WNDCLASSW {
                    lpfnWndProc: Some(fixture_proc),
                    hInstance: HINSTANCE(module.0),
                    lpszClassName: w!("Bald.OwnFrameFixture"),
                    ..Default::default()
                }),
                0
            );
            let hwnd = CreateWindowExW(
                Default::default(),
                w!("Bald.OwnFrameFixture"),
                w!("frame fixture"),
                WS_OVERLAPPEDWINDOW,
                -20000,
                -20000,
                400,
                300,
                None,
                None,
                Some(HINSTANCE(module.0)),
                None,
            )
            .unwrap();
            let expected = RECT {
                left: 10,
                top: 20,
                right: 410,
                bottom: 320,
            };
            let mut before = expected;
            SendMessageW(
                hwnd,
                WM_NCCALCSIZE,
                Some(WPARAM(0)),
                Some(LPARAM((&mut before as *mut RECT) as isize)),
            );
            assert_ne!(
                before, expected,
                "default FALSE path recreates native frame margins"
            );
            let value = hwnd.0 as usize;
            assert!(
                std::thread::spawn(move || install(HWND(value as *mut _)).is_err())
                    .join()
                    .unwrap()
            );
            install(hwnd).unwrap();
            let mut outer = RECT::default();
            let mut client = RECT::default();
            GetWindowRect(hwnd, &mut outer).unwrap();
            GetClientRect(hwnd, &mut client).unwrap();
            assert_eq!(
                (client.right, client.bottom),
                (outer.right - outer.left, outer.bottom - outer.top)
            );
            let mut simple = expected;
            SendMessageW(
                hwnd,
                WM_NCCALCSIZE,
                Some(WPARAM(0)),
                Some(LPARAM((&mut simple as *mut RECT) as isize)),
            );
            assert_eq!(simple, expected);
            let mut complex = NCCALCSIZE_PARAMS::default();
            complex.rgrc[0] = expected;
            SendMessageW(
                hwnd,
                WM_NCCALCSIZE,
                Some(WPARAM(1)),
                Some(LPARAM((&mut complex as *mut NCCALCSIZE_PARAMS) as isize)),
            );
            assert_eq!(complex.rgrc[0], expected);
            DestroyWindow(hwnd).unwrap();
        }
    }
}
