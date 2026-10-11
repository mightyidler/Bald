//! Preserve Bald's UI across internal restarts, without requesting focus.
use anyhow::Result;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use std::ffi::c_void;
use windows::Win32::{
    Foundation::{HWND, RECT},
    Graphics::Gdi::{
        GetMonitorInfoW, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect,
        MonitorFromWindow,
    },
    UI::WindowsAndMessaging::{
        GW_HWNDPREV, GWL_EXSTYLE, GetWindow, GetWindowLongW, GetWindowPlacement, GetWindowRect,
        GetWindowThreadProcessId, HWND_BOTTOM, HWND_TOP, IsIconic, IsWindow, IsWindowVisible,
        SW_HIDE, SW_SHOWMINNOACTIVE, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_NOSENDCHANGING,
        SWP_SHOWWINDOW, SetWindowPlacement, SetWindowPos, ShowWindow, WINDOWPLACEMENT,
        WS_EX_TOOLWINDOW,
    },
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct UiWindowState {
    visible: bool,
    minimized: bool,
    rect: [i32; 4],
    above: isize,
    above_pid: u32,
}

impl UiWindowState {
    pub fn visible(self) -> bool {
        self.visible
    }

    pub fn hidden(mut self) -> Self {
        self.visible = false;
        self.minimized = false;
        self
    }

    pub fn capture(hwnd: HWND) -> Result<Self> {
        unsafe {
            let visible = IsWindowVisible(hwnd).as_bool();
            let minimized = IsIconic(hwnd).as_bool();
            let mut rect = RECT::default();
            if minimized {
                let mut placement = WINDOWPLACEMENT {
                    length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
                    ..Default::default()
                };
                GetWindowPlacement(hwnd, &mut placement)?;
                rect = placement.rcNormalPosition;
                let (x, y) =
                    placement_offset(hwnd, MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST))?;
                rect = offset_rect(rect, x, y);
            } else {
                GetWindowRect(hwnd, &mut rect)?;
            }
            let above = GetWindow(hwnd, GW_HWNDPREV).unwrap_or_default();
            let mut above_pid = 0;
            GetWindowThreadProcessId(above, Some(&mut above_pid));
            Ok(Self {
                visible,
                minimized,
                rect: [rect.left, rect.top, rect.right, rect.bottom],
                above: above.0 as isize,
                above_pid,
            })
        }
    }

    pub fn argument(self) -> Result<String> {
        Ok(format!(
            "--ui-state={}",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&self)?)
        ))
    }

    pub fn from_argument(value: &str) -> Option<Self> {
        let bytes = URL_SAFE_NO_PAD
            .decode(value.strip_prefix("--ui-state=")?)
            .ok()?;
        let state: Self = serde_json::from_slice(&bytes).ok()?;
        let width = state.rect[2].checked_sub(state.rect[0])?;
        let height = state.rect[3].checked_sub(state.rect[1])?;
        (width > 0 && height > 0 && width <= 32768 && height <= 32768).then_some(state)
    }

    pub fn apply(self, hwnd: HWND) -> Result<()> {
        unsafe {
            let above = HWND(self.above as *mut c_void);
            let mut pid = 0;
            GetWindowThreadProcessId(above, Some(&mut pid));
            let anchor = if self.above == 0 {
                HWND_TOP
            } else if IsWindow(Some(above)).as_bool() && pid == self.above_pid {
                above
            } else {
                HWND_BOTTOM
            };
            if !self.visible {
                let _ = ShowWindow(hwnd, SW_HIDE);
            } else if self.minimized {
                let _ = ShowWindow(hwnd, SW_SHOWMINNOACTIVE);
                let mut placement = WINDOWPLACEMENT {
                    length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
                    ..Default::default()
                };
                GetWindowPlacement(hwnd, &mut placement)?;
                placement.rcNormalPosition = RECT {
                    left: self.rect[0],
                    top: self.rect[1],
                    right: self.rect[2],
                    bottom: self.rect[3],
                };
                let monitor =
                    MonitorFromRect(&placement.rcNormalPosition, MONITOR_DEFAULTTONEAREST);
                let (x, y) = placement_offset(hwnd, monitor)?;
                placement.rcNormalPosition = offset_rect(placement.rcNormalPosition, -x, -y);
                SetWindowPlacement(hwnd, &placement)?;
            }
            let mut flags = SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_NOSENDCHANGING;
            if self.visible && !self.minimized {
                flags |= SWP_SHOWWINDOW;
            }
            if self.minimized {
                flags |= windows::Win32::UI::WindowsAndMessaging::SWP_NOMOVE
                    | windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE;
            }
            SetWindowPos(
                hwnd,
                Some(anchor),
                self.rect[0],
                self.rect[1],
                self.rect[2] - self.rect[0],
                self.rect[3] - self.rect[1],
                flags,
            )?;
        }
        Ok(())
    }
}

fn placement_offset(hwnd: HWND, monitor: HMONITOR) -> Result<(i32, i32)> {
    unsafe {
        if GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0 {
            return Ok((0, 0));
        }
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        GetMonitorInfoW(monitor, &mut info).ok()?;
        Ok((
            info.rcWork.left - info.rcMonitor.left,
            info.rcWork.top - info.rcMonitor.top,
        ))
    }
}

fn offset_rect(rect: RECT, x: i32, y: i32) -> RECT {
    RECT {
        left: rect.left + x,
        top: rect.top + y,
        right: rect.right + x,
        bottom: rect.bottom + y,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn minimized_snapshots_use_screen_coordinates_for_recreation_with_top_or_left_taskbars() {
        for (x, y) in [(48, 0), (0, 40), (56, 44)] {
            let screen = RECT {
                left: -1700,
                top: 100,
                right: -1212,
                bottom: 820,
            };
            let placement = offset_rect(screen, -x, -y);
            let captured = offset_rect(placement, x, y);
            let snapshot = UiWindowState {
                visible: true,
                minimized: true,
                rect: [captured.left, captured.top, captured.right, captured.bottom],
                above: 0,
                above_pid: 0,
            };
            assert_eq!(snapshot.hidden().rect, [-1700, 100, -1212, 820]);
            assert_eq!(offset_rect(captured, -x, -y), placement);
        }
    }
    unsafe extern "system" fn fixture_proc(
        hwnd: HWND,
        message: u32,
        wparam: windows::Win32::Foundation::WPARAM,
        lparam: windows::Win32::Foundation::LPARAM,
    ) -> windows::Win32::Foundation::LRESULT {
        unsafe {
            windows::Win32::UI::WindowsAndMessaging::DefWindowProcW(hwnd, message, wparam, lparam)
        }
    }
    #[test]
    fn restart_argument_roundtrips_all_visibility_states_and_z_order() {
        for (visible, minimized) in [(false, false), (true, false), (true, true)] {
            let state = UiWindowState {
                visible,
                minimized,
                rect: [-1200, 80, -712, 800],
                above: 1234,
                above_pid: 5678,
            };
            assert_eq!(
                UiWindowState::from_argument(&state.argument().unwrap()),
                Some(state)
            );
        }
        assert!(UiWindowState::from_argument("--ui-state=invalid").is_none());
    }

    #[test]
    fn native_restart_keeps_visibility_minimization_z_order_and_focus() {
        use windows::{
            Win32::{
                Foundation::HINSTANCE,
                System::LibraryLoader::GetModuleHandleW,
                UI::WindowsAndMessaging::{
                    CreateWindowExW, DestroyWindow, GetForegroundWindow, RegisterClassW, WNDCLASSW,
                    WS_OVERLAPPEDWINDOW,
                },
            },
            core::w,
        };
        unsafe {
            let module = HINSTANCE(GetModuleHandleW(None).unwrap().0);
            assert_ne!(
                RegisterClassW(&WNDCLASSW {
                    lpfnWndProc: Some(fixture_proc),
                    hInstance: module,
                    lpszClassName: w!("Bald.UiStateFixture"),
                    ..Default::default()
                }),
                0
            );
            let make = || {
                CreateWindowExW(
                    Default::default(),
                    w!("Bald.UiStateFixture"),
                    w!("fixture"),
                    WS_OVERLAPPEDWINDOW,
                    -24000,
                    -23000,
                    488,
                    720,
                    None,
                    None,
                    Some(module),
                    None,
                )
                .unwrap()
            };
            for (visible, minimized) in [(false, false), (true, false), (true, true)] {
                let anchor = make();
                let old = make();
                let replacement = make();
                let foreground = GetForegroundWindow();
                let snapshot = UiWindowState {
                    visible,
                    minimized,
                    rect: [-24000, -23000, -23512, -22280],
                    above: anchor.0 as isize,
                    above_pid: windows::Win32::System::Threading::GetCurrentProcessId(),
                };
                snapshot.apply(old).unwrap();
                let captured = UiWindowState::capture(old).unwrap();
                captured.apply(replacement).unwrap();
                assert_eq!(IsWindowVisible(replacement).as_bool(), visible);
                assert_eq!(IsIconic(replacement).as_bool(), minimized);
                assert_eq!(GetWindow(replacement, GW_HWNDPREV).unwrap(), anchor);
                assert_eq!(GetForegroundWindow(), foreground);
                assert_eq!(
                    UiWindowState::capture(replacement).unwrap().rect,
                    captured.rect
                );
                DestroyWindow(replacement).unwrap();
                DestroyWindow(old).unwrap();
                DestroyWindow(anchor).unwrap();
            }
        }
    }
}
