// Window-style behavior is derived from ihateborders (GPL-3.0),
// https://github.com/Z1xus/ihateborders, Copyright its contributors.

use std::{
    collections::HashMap,
    ffi::c_void,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
};

use crate::rules::DragMode;
use anyhow::{Result, bail};
use windows::{
    Win32::{
        Foundation::{
            CloseHandle, GetLastError, HANDLE, HWND, LPARAM, POINT, RECT, SetLastError, WIN32_ERROR,
        },
        Graphics::Dwm::{
            DWMNCRP_DISABLED, DWMNCRP_ENABLED, DWMWA_NCRENDERING_ENABLED, DWMWA_NCRENDERING_POLICY,
            DwmGetWindowAttribute, DwmSetWindowAttribute,
        },
        Graphics::Gdi::{
            ClientToScreen, CombineRgn, CreateRectRgn, DeleteObject, GetMonitorInfoW, GetWindowRgn,
            HRGN, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow, RDW_ALLCHILDREN,
            RDW_FRAME, RDW_INVALIDATE, RGN_COPY, RedrawWindow, SetWindowRgn,
        },
        Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation},
        System::Threading::{
            GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
        },
        UI::{
            Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON},
            WindowsAndMessaging::{
                EnumWindows, GW_OWNER, GWL_EXSTYLE, GWL_STYLE, GetClassNameW, GetClientRect,
                GetForegroundWindow, GetPropW, GetWindow, GetWindowLongW, GetWindowPlacement,
                GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow,
                IsWindowVisible, IsZoomed, RemovePropW, SET_WINDOW_POS_FLAGS, SWP_ASYNCWINDOWPOS,
                SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOREDRAW,
                SWP_NOSENDCHANGING, SWP_NOSIZE, SWP_NOZORDER, SetPropW, SetWindowLongW,
                SetWindowPlacement, SetWindowPos, WINDOWPLACEMENT, WS_BORDER, WS_CAPTION,
                WS_DLGFRAME, WS_EX_CLIENTEDGE, WS_EX_DLGMODALFRAME, WS_EX_STATICEDGE,
                WS_EX_TOOLWINDOW, WS_EX_WINDOWEDGE, WS_MINIMIZE, WS_SYSMENU, WS_THICKFRAME,
            },
        },
    },
    core::w,
};

pub struct ExecutableIcon {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[cfg(test)]
use windows::Win32::Foundation::WPARAM;

#[path = "executable_icon_resources.rs"]
mod icon_resources;

pub fn executable_icon(path: &str) -> Option<ExecutableIcon> {
    icon_resources::from_executable(path)
        .or_else(|| high_resolution_executable_icon(path))
        .or_else(|| legacy_executable_icon(path))
}

fn crop_transparent_padding(icon: ExecutableIcon) -> ExecutableIcon {
    let mut left = icon.width;
    let mut top = icon.height;
    let mut right = 0;
    let mut bottom = 0;
    for y in 0..icon.height {
        for x in 0..icon.width {
            if icon.rgba[((y * icon.width + x) * 4 + 3) as usize] > 8 {
                left = left.min(x);
                top = top.min(y);
                right = right.max(x + 1);
                bottom = bottom.max(y + 1);
            }
        }
    }
    if left >= right
        || top >= bottom
        || (left == 0 && top == 0 && right == icon.width && bottom == icon.height)
    {
        return icon;
    }
    let width = right - left;
    let height = bottom - top;
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in top..bottom {
        let start = ((y * icon.width + left) * 4) as usize;
        rgba.extend_from_slice(&icon.rgba[start..start + (width * 4) as usize]);
    }
    ExecutableIcon {
        width,
        height,
        rgba,
    }
}

fn high_resolution_executable_icon(path: &str) -> Option<ExecutableIcon> {
    use std::mem::size_of;
    use windows::{
        Win32::{
            Foundation::SIZE,
            Graphics::Gdi::{
                BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS,
                DeleteDC, DeleteObject, GetDIBits, GetObjectW, HGDIOBJ,
            },
            System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize},
            UI::Shell::{
                IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK,
                SIIGBF_ICONONLY,
            },
        },
        core::PCWSTR,
    };

    let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    struct Apartment(bool);
    impl Drop for Apartment {
        fn drop(&mut self) {
            if self.0 {
                unsafe {
                    CoUninitialize();
                }
            }
        }
    }
    unsafe {
        let _apartment = Apartment(CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok());
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None).ok()?;
        let bitmap = factory
            .GetImage(
                SIZE { cx: 256, cy: 256 },
                SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK,
            )
            .ok()?;
        let mut details = BITMAP::default();
        if GetObjectW(
            HGDIOBJ(bitmap.0),
            size_of::<BITMAP>() as i32,
            Some((&mut details as *mut BITMAP).cast()),
        ) == 0
        {
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            return None;
        }
        let width = details.bmWidth.max(1) as u32;
        let height = details.bmHeight.max(1) as u32;
        let mut info = BITMAPINFO::default();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };
        let dc = CreateCompatibleDC(None);
        if dc.0.is_null() {
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            return None;
        }
        let mut rgba = vec![0u8; (width * height * 4) as usize];
        let lines = GetDIBits(
            dc,
            bitmap,
            0,
            height,
            Some(rgba.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        );
        let _ = DeleteDC(dc);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        if lines == 0 {
            return None;
        }
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        Some(crop_transparent_padding(ExecutableIcon {
            width,
            height,
            rgba,
        }))
    }
}

fn legacy_executable_icon(path: &str) -> Option<ExecutableIcon> {
    use std::{ffi::c_void, mem::size_of, ptr::copy_nonoverlapping};
    use windows::{
        Win32::{
            Graphics::Gdi::{
                BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection,
                DIB_RGB_COLORS, DeleteDC, DeleteObject, HGDIOBJ, SelectObject,
            },
            UI::{
                Shell::{SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGetFileInfoW},
                WindowsAndMessaging::{DI_NORMAL, DestroyIcon, DrawIconEx},
            },
        },
        core::PCWSTR,
    };

    const SIZE: i32 = 32;
    let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    let mut info = SHFILEINFOW::default();
    let found = unsafe {
        SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            Default::default(),
            Some(&mut info),
            size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        )
    };
    if found == 0 || info.hIcon.0.is_null() {
        return None;
    }

    let result = unsafe {
        let dc = CreateCompatibleDC(None);
        if dc.0.is_null() {
            let _ = DestroyIcon(info.hIcon);
            return None;
        }
        let mut bitmap_info = BITMAPINFO::default();
        bitmap_info.bmiHeader = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: SIZE,
            biHeight: -SIZE,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let bitmap = CreateDIBSection(Some(dc), &bitmap_info, DIB_RGB_COLORS, &mut bits, None, 0);
        let Ok(bitmap) = bitmap else {
            let _ = DeleteDC(dc);
            let _ = DestroyIcon(info.hIcon);
            return None;
        };
        let old = SelectObject(dc, HGDIOBJ(bitmap.0));
        let drawn = DrawIconEx(dc, 0, 0, info.hIcon, SIZE, SIZE, 0, None, DI_NORMAL).is_ok();
        let mut bgra = vec![0u8; (SIZE * SIZE * 4) as usize];
        if drawn && !bits.is_null() {
            copy_nonoverlapping(bits.cast::<u8>(), bgra.as_mut_ptr(), bgra.len());
        }
        SelectObject(dc, old);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(dc);
        let _ = DestroyIcon(info.hIcon);
        drawn.then(|| {
            for pixel in bgra.chunks_exact_mut(4) {
                pixel.swap(0, 2);
                if pixel[3] == 0 && (pixel[0] != 0 || pixel[1] != 0 || pixel[2] != 0) {
                    pixel[3] = 255;
                }
            }
            ExecutableIcon {
                width: SIZE as u32,
                height: SIZE as u32,
                rgba: bgra,
            }
        })
    };
    result
}

const BORDER_STYLES: u32 = WS_BORDER.0 | WS_CAPTION.0 | WS_THICKFRAME.0 | WS_DLGFRAME.0;
const DRAG_HEIGHT: i32 = 16;
const BORDER_EX_STYLES: u32 =
    WS_EX_DLGMODALFRAME.0 | WS_EX_WINDOWEDGE.0 | WS_EX_CLIENTEDGE.0 | WS_EX_STATICEDGE.0;
const QUIET_POSITION: windows::Win32::UI::WindowsAndMessaging::SET_WINDOW_POS_FLAGS =
    windows::Win32::UI::WindowsAndMessaging::SET_WINDOW_POS_FLAGS(
        SWP_NOACTIVATE.0 | SWP_NOZORDER.0 | SWP_NOOWNERZORDER.0 | SWP_NOSENDCHANGING.0,
    );

#[derive(Debug, Clone)]
pub struct WindowInfo {
    pub hwnd: isize,
    pub title: String,
    pub executable_name: String,
    pub executable_path: Option<String>,
    pub class_name: String,
    pub is_borderless: bool,
}

#[derive(Clone, Default)]
pub struct WindowController {
    operations: Arc<Mutex<()>>,
    original_windows: Arc<Mutex<HashMap<isize, OriginalWindowState>>>,
    shutting_down: Arc<AtomicBool>,
    drag_rules: Arc<Mutex<HashMap<isize, DragRule>>>,
}

struct DragRule {
    process_id: u32,
    mode: DragMode,
    stalled: bool,
    in_flight: bool,
}

impl DragRule {
    fn allowed(&self) -> bool {
        !self.stalled && self.mode == DragMode::Enabled
    }
}

#[derive(Debug, Clone, Copy)]
struct OriginalWindowState {
    process_id: u32,
    style: i32,
    ex_style: i32,
    native_rendering: Option<bool>,
    clipped: bool,
    native_region: bool,
    original_region: Option<isize>,
    clipped_size: Option<(i32, i32)>,
    clipped_content_size: Option<(i32, i32)>,
    frame_insets: RECT,
    restoring: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct UserDrag {
    hwnd: isize,
    process_id: u32,
    window_origin: POINT,
    cursor_origin: POINT,
    pending: Option<PendingDragMove>,
    position_flags: SET_WINDOW_POS_FLAGS,
}

#[derive(Clone, Copy)]
struct PendingDragMove {
    x: i32,
    y: i32,
    started: std::time::Instant,
}

impl WindowController {
    pub fn configure_drag_rules(&self, rules: Vec<(isize, DragMode)>) {
        let live: HashMap<_, _> = rules
            .into_iter()
            .map(|(hwnd, mode)| (hwnd, (window_process_id(HWND(hwnd as *mut c_void)), mode)))
            .collect();
        let mut states = self.drag_rules.lock().unwrap();
        states.retain(|hwnd, state| {
            live.get(hwnd)
                .is_some_and(|(pid, _)| *pid == state.process_id)
        });
        for (hwnd, (process_id, mode)) in live {
            let state = states.entry(hwnd).or_insert(DragRule {
                process_id,
                mode,
                stalled: false,
                in_flight: false,
            });
            if state.mode != mode {
                state.mode = mode;
                state.stalled = false;
            }
        }
    }

    fn drag_allowed(&self, hwnd: isize) -> bool {
        self.drag_rules
            .try_lock()
            .is_ok_and(|rules| rules.get(&hwnd).is_some_and(DragRule::allowed))
    }

    fn can_begin_drag(&self, hwnd: isize) -> bool {
        self.drag_rules.try_lock().is_ok_and(|rules| {
            rules
                .get(&hwnd)
                .is_some_and(|rule| rule.allowed() && !rule.in_flight)
        })
    }

    pub fn set_window_drag_mode(&self, hwnd: isize, mode: DragMode) {
        if let Some(rule) = self.drag_rules.lock().unwrap().get_mut(&hwnd) {
            if rule.mode != mode {
                rule.stalled = false;
            }
            rule.mode = mode;
        }
    }

    pub fn center_blocked_window(&self, hwnd_value: isize) -> Result<bool> {
        let hwnd = HWND(hwnd_value as *mut c_void);
        let process_id = window_process_id(hwnd);
        let mut monitor = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        unsafe {
            if !GetMonitorInfoW(
                MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST),
                &mut monitor,
            )
            .as_bool()
            {
                bail!("cannot determine the window's monitor");
            }
        }
        self.center_blocked_in_rect(hwnd_value, process_id, monitor.rcMonitor)
    }

    fn center_blocked_in_rect(
        &self,
        hwnd_value: isize,
        process_id: u32,
        bounds: RECT,
    ) -> Result<bool> {
        let _operation = self.operations.lock().unwrap();
        if self.shutting_down.load(Ordering::Acquire) {
            return Ok(false);
        }
        // Never queue a center behind an outstanding drag request.
        if let Some(rule) = self.drag_rules.lock().unwrap().get(&hwnd_value) {
            if rule.process_id != process_id || rule.mode != DragMode::Disabled || rule.in_flight {
                bail!(
                    "window movement pending or not blocked; try again after releasing the mouse"
                );
            }
        }
        let hwnd = HWND(hwnd_value as *mut c_void);
        if is_native_region_game(hwnd)
            && !self
                .original_windows
                .lock()
                .unwrap()
                .get(&hwnd_value)
                .is_some_and(|state| state.native_region)
        {
            return Ok(false);
        }
        unsafe {
            if !IsWindow(Some(hwnd)).as_bool()
                || window_process_id(hwnd) != process_id
                || !IsWindowVisible(hwnd).as_bool()
                || IsIconic(hwnd).as_bool()
                || IsZoomed(hwnd).as_bool()
            {
                return Ok(false);
            }
            if requires_elevation(hwnd) {
                bail!("elevation_required");
            }
            let mut rect = RECT::default();
            GetWindowRect(hwnd, &mut rect)?;
            let surface = self.drag_surface(hwnd).unwrap_or(rect);
            let width = surface.right - surface.left;
            let height = surface.bottom - surface.top;
            let x = bounds.left + ((bounds.right - bounds.left) - width) / 2
                - (surface.left - rect.left);
            let y =
                bounds.top + ((bounds.bottom - bounds.top) - height) / 2 - (surface.top - rect.top);
            if (rect.left, rect.top) == (x, y) {
                return Ok(false);
            }
            SetWindowPos(hwnd, None, x, y, 0, 0, user_drag_position_flags(hwnd))?;
            let started = std::time::Instant::now();
            loop {
                if !IsWindow(Some(hwnd)).as_bool() || window_process_id(hwnd) != process_id {
                    bail!("window closed while centering");
                }
                GetWindowRect(hwnd, &mut rect)?;
                if (rect.left, rect.top) == (x, y) {
                    return Ok(true);
                }
                if started.elapsed() >= std::time::Duration::from_millis(100) {
                    bail!("window did not accept the centered position");
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
    }

    pub fn make_borderless(&self, hwnd_value: isize) -> Result<bool> {
        let hwnd = HWND(hwnd_value as *mut c_void);
        if is_native_region_game(hwnd) {
            let _operation = self.operations.lock().unwrap();
            if self.shutting_down.load(Ordering::Acquire) {
                return Ok(false);
            }
            unsafe {
                if !IsWindow(Some(hwnd)).as_bool() {
                    bail!("window no longer exists");
                }
                if requires_elevation(hwnd) {
                    bail!("elevation_required");
                }
                if IsIconic(hwnd).as_bool() || !IsWindowVisible(hwnd).as_bool() {
                    return Ok(false);
                }
            }
            let stale = self
                .original_windows
                .lock()
                .unwrap()
                .get(&hwnd_value)
                .is_some_and(|state| {
                    state.process_id != window_process_id(hwnd)
                        || unsafe { GetPropW(hwnd, w!("Bald.OriginalStyle")).0.is_null() }
                });
            if stale {
                self.forget_window(hwnd_value);
            }
            return self.clip_native_frame(hwnd_value);
        }
        self.make_borderless_style(hwnd_value)
    }

    fn make_borderless_style(&self, hwnd_value: isize) -> Result<bool> {
        let _operation = self.operations.lock().unwrap();
        if self.shutting_down.load(Ordering::Acquire) {
            return Ok(false);
        }
        let hwnd = HWND(hwnd_value as *mut c_void);
        unsafe {
            if !IsWindow(Some(hwnd)).as_bool() {
                bail!("window no longer exists");
            }
            if requires_elevation(hwnd) {
                bail!("elevation_required");
            }
            if IsIconic(hwnd).as_bool() || !IsWindowVisible(hwnd).as_bool() {
                return Ok(false);
            }
            let stale = self
                .original_windows
                .lock()
                .unwrap()
                .get(&hwnd_value)
                .is_some_and(|state| {
                    state.process_id != window_process_id(hwnd)
                        || GetPropW(hwnd, w!("Bald.OriginalStyle")).0.is_null()
                });
            if stale {
                self.forget_window(hwnd_value);
            }
            if let Some(state) = self.original_windows.lock().unwrap().get_mut(&hwnd_value) {
                state.restoring = false;
            }
            let current = GetWindowLongW(hwnd, GWL_STYLE);
            let current_ex = GetWindowLongW(hwnd, GWL_EXSTYLE);
            if current as u32 & BORDER_STYLES == 0 && current_ex as u32 & BORDER_EX_STYLES == 0 {
                return Ok(false);
            }

            // Keep the existing client area stable in screen coordinates. Some games cache
            // their render/input surface and otherwise end up drawing into the former caption
            // area while still hit-testing against the old client origin.
            let mut window_rect = RECT::default();
            GetWindowRect(hwnd, &mut window_rect)?;
            let mut client_rect = RECT::default();
            GetClientRect(hwnd, &mut client_rect)?;
            let mut client_origin = POINT {
                x: client_rect.left,
                y: client_rect.top,
            };
            ClientToScreen(hwnd, &mut client_origin).ok()?;
            let client_width = client_rect.right - client_rect.left;
            let client_height = client_rect.bottom - client_rect.top;
            if client_width <= 0 || client_height <= 0 {
                return Ok(false);
            }
            if !self
                .original_windows
                .lock()
                .unwrap()
                .contains_key(&hwnd_value)
                && !GetPropW(hwnd, w!("Bald.OriginalStyle")).0.is_null()
                && GetPropW(hwnd, w!("Bald.ClippedFrame")).0.is_null()
                && current as u32 & WS_CAPTION.0 == WS_CAPTION.0
                && client_origin.y <= window_rect.top
            {
                // A prior process failed to restore this caption. Its original
                // rendering state is unknown; do not snapshot the broken state.
                bail!(
                    "previous native caption is not restored; reapply windowed resolution before enabling"
                );
            }
            // Old region-based builds can leave markers after a failed exit.
            // Only clear them after the game has independently restored its
            // real native caption; never crop or guess a broken legacy frame.
            if !self
                .original_windows
                .lock()
                .unwrap()
                .contains_key(&hwnd_value)
                && !GetPropW(hwnd, w!("Bald.ClippedFrame")).0.is_null()
            {
                let region = CreateRectRgn(0, 0, 0, 0);
                if region.0.is_null() {
                    bail!("inspect previous frame region failed");
                }
                let has_region = GetWindowRgn(hwnd, region).0 != 0;
                let _ = DeleteObject(region.into());
                if has_region || client_origin.y <= window_rect.top {
                    bail!(
                        "legacy game frame is not restored; reapply windowed resolution before enabling"
                    );
                }
                let _ = RemovePropW(hwnd, w!("Bald.ClippedFrame"));
                let _ = RemovePropW(hwnd, w!("Bald.OriginalStyle"));
                let _ = RemovePropW(hwnd, w!("Bald.OriginalExStyle"));
            }

            self.original_windows
                .lock()
                .unwrap()
                .entry(hwnd_value)
                .or_insert(OriginalWindowState {
                    process_id: window_process_id(hwnd),
                    style: current,
                    ex_style: current_ex,
                    native_rendering: native_frame_rendering(hwnd),
                    clipped: false,
                    native_region: false,
                    original_region: None,
                    clipped_size: None,
                    clipped_content_size: None,
                    frame_insets: RECT {
                        left: client_origin.x - window_rect.left,
                        top: client_origin.y - window_rect.top,
                        right: window_rect.right - client_origin.x - client_width,
                        bottom: window_rect.bottom - client_origin.y - client_height,
                    },
                    restoring: false,
                });
            let _ = SetPropW(
                hwnd,
                w!("Bald.OriginalStyle"),
                Some(HANDLE(current as isize as *mut c_void)),
            );
            let _ = SetPropW(
                hwnd,
                w!("Bald.OriginalExStyle"),
                Some(HANDLE(current_ex as isize as *mut c_void)),
            );
            SetLastError(WIN32_ERROR(0));
            let previous = if current as u32 & BORDER_STYLES != 0 {
                SetWindowLongW(hwnd, GWL_STYLE, (current as u32 & !BORDER_STYLES) as i32)
            } else {
                current
            };
            let error = GetLastError();
            if previous == 0 {
                if error.0 != 0 {
                    self.original_windows.lock().unwrap().remove(&hwnd_value);
                    if error.0 == 5 {
                        bail!("elevation_required");
                    }
                    bail!("SetWindowLongW failed: {}", error.0);
                }
            }
            if current_ex as u32 & BORDER_EX_STYLES != 0
                && let Err(error) = set_window_long_checked(
                    hwnd,
                    GWL_EXSTYLE,
                    (current_ex as u32 & !BORDER_EX_STYLES) as i32,
                )
            {
                let _ = set_style_checked(hwnd, current);
                return Err(error);
            }
            let flags = SWP_FRAMECHANGED | QUIET_POSITION | owner_frame_dispatch(hwnd);
            let positioned = SetWindowPos(
                hwnd,
                None,
                client_origin.x,
                client_origin.y,
                client_width,
                client_height,
                flags,
            );
            positioned?;
            if is_native_region_game(hwnd) {
                wait_for_frame_geometry(
                    hwnd,
                    client_origin.x,
                    client_origin.y,
                    client_width,
                    client_height,
                )?;
            }
            Ok(true)
        }
    }

    // Keep Maple's logical caption: removing it redirects client clicks into
    // its broken internal drag path. DWM frame painting must be disabled before
    // clipping; a region alone does not hide the composed modern caption.
    fn clip_native_frame(&self, hwnd_value: isize) -> Result<bool> {
        use windows::Win32::Graphics::Gdi::EqualRgn;
        let hwnd = HWND(hwnd_value as *mut c_void);
        unsafe {
            if GetAsyncKeyState(VK_LBUTTON.0 as i32) < 0 {
                return Ok(false);
            }
            let saved = self
                .original_windows
                .lock()
                .unwrap()
                .get(&hwnd_value)
                .copied();
            if saved.is_some_and(|state| !state.native_region) {
                bail!("restore the previous managed frame before using native clipping");
            }
            if saved.is_none() && !GetPropW(hwnd, w!("Bald.NativeFrameRegion")).0.is_null() {
                bail!(
                    "previous native region is still active; restore the prior Bald session first"
                );
            }
            if saved.is_some_and(|state| state.restoring) {
                refresh_native_region(hwnd, window_process_id(hwnd), None)?;
            }
            let mut outer = RECT::default();
            let mut client = RECT::default();
            let mut origin = POINT::default();
            GetWindowRect(hwnd, &mut outer)?;
            GetClientRect(hwnd, &mut client)?;
            ClientToScreen(hwnd, &mut origin).ok()?;
            if origin.y <= outer.top || client.right <= 0 || client.bottom <= 0 {
                bail!("native game frame must be restored before applying");
            }
            let desired = CreateRectRgn(
                origin.x - outer.left,
                origin.y - outer.top,
                origin.x - outer.left + client.right,
                origin.y - outer.top + client.bottom,
            );
            let previous = CreateRectRgn(0, 0, 0, 0);
            if desired.0.is_null() || previous.0.is_null() {
                let _ = DeleteObject(desired.into());
                let _ = DeleteObject(previous.into());
                bail!("create native game region failed");
            }
            let has_region = GetWindowRgn(hwnd, previous).0 != 0;
            let whole = CreateRectRgn(0, 0, outer.right - outer.left, outer.bottom - outer.top);
            let rendering = native_frame_rendering(hwnd);
            let keep_region =
                has_region && !(rendering == Some(true) && EqualRgn(previous, whole).as_bool());
            let _ = DeleteObject(whole.into());
            let managed = self
                .original_windows
                .lock()
                .unwrap()
                .contains_key(&hwnd_value);
            if managed
                && !saved.is_some_and(|state| state.restoring)
                && has_region
                && EqualRgn(previous, desired).as_bool()
                && rendering == Some(false)
            {
                let _ = DeleteObject(previous.into());
                let _ = DeleteObject(desired.into());
                return Ok(false);
            }
            if !managed {
                let style = GetWindowLongW(hwnd, GWL_STYLE);
                self.original_windows.lock().unwrap().insert(
                    hwnd_value,
                    OriginalWindowState {
                        process_id: window_process_id(hwnd),
                        style,
                        ex_style: GetWindowLongW(hwnd, GWL_EXSTYLE),
                        native_rendering: native_frame_rendering(hwnd),
                        clipped: true,
                        native_region: true,
                        // A full native frame region is equivalent to no clip.
                        // Reinstalling it as a custom region can disable DWM
                        // rendering on the next restore cycle.
                        original_region: keep_region.then_some(previous.0 as isize),
                        clipped_size: None,
                        clipped_content_size: None,
                        frame_insets: RECT::default(),
                        restoring: true,
                    },
                );
                SetPropW(
                    hwnd,
                    w!("Bald.OriginalStyle"),
                    Some(HANDLE(style as isize as *mut c_void)),
                )?;
            }
            if managed || !keep_region {
                let _ = DeleteObject(previous.into());
            }
            if let Some(state) = self.original_windows.lock().unwrap().get_mut(&hwnd_value) {
                state.restoring = true;
                state.frame_insets = RECT {
                    left: origin.x - outer.left,
                    top: origin.y - outer.top,
                    right: outer.right - origin.x - client.right,
                    bottom: outer.bottom - origin.y - client.bottom,
                };
            }
            // A changed clip alone does not require rebuilding the native frame.
            // Rebuilding can expose the logical caption retained for Maple's input.
            if rendering != Some(false) {
                let policy = DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_NCRENDERING_POLICY,
                    &DWMNCRP_DISABLED as *const _ as *const c_void,
                    std::mem::size_of_val(&DWMNCRP_DISABLED) as u32,
                );
                if let Err(error) = policy {
                    let _ = DeleteObject(desired.into());
                    return Err(error.into());
                }
                // Owners can replace a region during a policy frame change, so
                // complete this notification before installing the final clip.
                let flags = SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | QUIET_POSITION;
                let positioned = SetWindowPos(hwnd, None, 0, 0, 0, 0, flags);
                if let Err(error) = positioned {
                    let _ = DeleteObject(desired.into());
                    return Err(error.into());
                }
            } else {
            }
            if SetWindowRgn(hwnd, Some(desired), false) == 0 {
                let error = GetLastError();
                let _ = DeleteObject(desired.into());
                bail!("set native game region failed: {}", error.0);
            }
            let mut after_client = RECT::default();
            let mut after_origin = POINT::default();
            GetClientRect(hwnd, &mut after_client)?;
            ClientToScreen(hwnd, &mut after_origin).ok()?;
            if (
                after_client.right,
                after_client.bottom,
                after_origin.x,
                after_origin.y,
            ) != (client.right, client.bottom, origin.x, origin.y)
            {
                refresh_native_region(
                    hwnd,
                    window_process_id(hwnd),
                    Some((client.right, client.bottom)),
                )?;
            }
            if let Some(state) = self.original_windows.lock().unwrap().get_mut(&hwnd_value) {
                state.restoring = false;
                state.clipped_size = Some((outer.right - outer.left, outer.bottom - outer.top));
                state.clipped_content_size = Some((client.right, client.bottom));
            }
            SetPropW(
                hwnd,
                w!("Bald.NativeFrameRegion"),
                Some(HANDLE(std::ptr::dangling_mut())),
            )?;
            let _ = RedrawWindow(
                Some(hwnd),
                None,
                None,
                RDW_INVALIDATE | RDW_FRAME | RDW_ALLCHILDREN,
            );
            Ok(true)
        }
    }

    #[cfg(test)]
    fn clip_game_frame(&self, hwnd_value: isize) -> Result<bool> {
        use windows::Win32::Graphics::Gdi::EqualRgn;
        let hwnd = HWND(hwnd_value as *mut c_void);
        unsafe {
            // Never fight the native move loop with a frame/size update.
            if GetAsyncKeyState(VK_LBUTTON.0 as i32) < 0 {
                return Ok(false);
            }
            let mut window = RECT::default();
            let mut client = RECT::default();
            let mut origin = POINT::default();
            GetWindowRect(hwnd, &mut window)?;
            GetClientRect(hwnd, &mut client)?;
            ClientToScreen(hwnd, &mut origin).ok()?;
            if client.right <= 0 || client.bottom <= 0 {
                return Ok(false);
            }
            let left = origin.x - window.left;
            let top = origin.y - window.top;
            let desired = CreateRectRgn(left, top, left + client.right, top + client.bottom);
            let previous = CreateRectRgn(0, 0, 0, 0);
            if desired.0.is_null() || previous.0.is_null() {
                let _ = DeleteObject(desired.into());
                let _ = DeleteObject(previous.into());
                bail!("create game frame region failed");
            }
            let has_region = GetWindowRgn(hwnd, previous).0 != 0;
            let save_existing_region =
                has_region && GetPropW(hwnd, w!("Bald.ClippedFrame")).0.is_null();
            if has_region
                && EqualRgn(previous, desired).as_bool()
                && self
                    .original_windows
                    .lock()
                    .unwrap()
                    .contains_key(&hwnd_value)
            {
                let _ = DeleteObject(previous.into());
                let _ = DeleteObject(desired.into());
                return Ok(false);
            }
            // Do not paint the intermediate expanded viewport. Redraw only
            // after both the final content size and region have been applied.
            if SetWindowRgn(hwnd, Some(desired), false) == 0 {
                let error = GetLastError();
                let _ = DeleteObject(previous.into());
                let _ = DeleteObject(desired.into());
                if error.0 == 5 {
                    bail!("elevation_required");
                }
                bail!("SetWindowRgn failed: {}", error.0);
            }
            // On success Windows owns desired; previous remains ours until restore.
            let mut windows = self.original_windows.lock().unwrap();
            if let std::collections::hash_map::Entry::Vacant(entry) = windows.entry(hwnd_value) {
                let style = GetWindowLongW(hwnd, GWL_STYLE);
                entry.insert(OriginalWindowState {
                    process_id: window_process_id(hwnd),
                    style,
                    ex_style: GetWindowLongW(hwnd, GWL_EXSTYLE),
                    native_rendering: native_frame_rendering(hwnd),
                    clipped: true,
                    native_region: false,
                    original_region: save_existing_region.then_some(previous.0 as isize),
                    clipped_size: Some((window.right - window.left, window.bottom - window.top)),
                    clipped_content_size: Some((client.right, client.bottom)),
                    frame_insets: RECT {
                        left,
                        top,
                        right: window.right - window.left - left - client.right,
                        bottom: window.bottom - window.top - top - client.bottom,
                    },
                    restoring: false,
                });
                if !save_existing_region {
                    let _ = DeleteObject(previous.into());
                }
                let _ = SetPropW(
                    hwnd,
                    w!("Bald.OriginalStyle"),
                    Some(HANDLE(style as isize as *mut c_void)),
                );
            } else {
                let _ = DeleteObject(previous.into());
                if left != 0 || top != 0 {
                    if let Some(state) = windows.get_mut(&hwnd_value) {
                        state.frame_insets = RECT {
                            left,
                            top,
                            right: window.right - window.left - left - client.right,
                            bottom: window.bottom - window.top - top - client.bottom,
                        };
                        state.clipped_content_size = Some((client.right, client.bottom));
                    }
                }
            }
            let _ = SetPropW(
                hwnd,
                w!("Bald.ClippedFrame"),
                Some(HANDLE(1usize as *mut c_void)),
            );
            drop(windows);
            // Maple expands its client area to the whole outer rectangle when
            // SetWindowRgn removes the themed non-client area. Preserve the
            // measured content size, not a hard-coded resolution or aspect ratio.
            // Resolution changes recreate the native client margins too. Only
            // compensate this measured transition, never a plain position change.
            let mut after_client = RECT::default();
            GetClientRect(hwnd, &mut after_client)?;
            if (left != 0 || top != 0)
                && after_client.right == window.right - window.left
                && after_client.bottom == window.bottom - window.top
            {
                SetWindowPos(
                    hwnd,
                    None,
                    origin.x,
                    origin.y,
                    client.right,
                    client.bottom,
                    SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOREDRAW,
                )?;
                let mut normalized_window = RECT::default();
                let mut normalized_client = RECT::default();
                let mut normalized_origin = POINT::default();
                GetWindowRect(hwnd, &mut normalized_window)?;
                GetClientRect(hwnd, &mut normalized_client)?;
                ClientToScreen(hwnd, &mut normalized_origin).ok()?;
                let x = normalized_origin.x - normalized_window.left;
                let y = normalized_origin.y - normalized_window.top;
                let region = CreateRectRgn(
                    x,
                    y,
                    x + normalized_client.right,
                    y + normalized_client.bottom,
                );
                if region.0.is_null() {
                    bail!("create normalized game frame region failed");
                }
                if SetWindowRgn(hwnd, Some(region), true) == 0 {
                    let _ = DeleteObject(region.into());
                    bail!("set normalized game frame region failed");
                }
                if let Some(state) = self.original_windows.lock().unwrap().get_mut(&hwnd_value) {
                    state.clipped_size = Some((
                        normalized_window.right - normalized_window.left,
                        normalized_window.bottom - normalized_window.top,
                    ));
                    state.clipped_content_size =
                        Some((normalized_client.right, normalized_client.bottom));
                }
            } else if let Some(state) = self.original_windows.lock().unwrap().get_mut(&hwnd_value) {
                state.clipped_size = Some((window.right - window.left, window.bottom - window.top));
                state.clipped_content_size = Some((client.right, client.bottom));
            }
            let _ = RedrawWindow(
                Some(hwnd),
                None,
                None,
                RDW_INVALIDATE | RDW_FRAME | RDW_ALLCHILDREN,
            );
            Ok(true)
        }
    }

    pub fn restore_borders(&self, hwnd_value: isize) -> Result<bool> {
        let _operation = self.operations.lock().unwrap();
        let hwnd = HWND(hwnd_value as *mut c_void);
        unsafe {
            let original = self
                .original_windows
                .lock()
                .unwrap()
                .get(&hwnd_value)
                .copied();
            let saved_style = GetPropW(hwnd, w!("Bald.OriginalStyle"));
            if !IsWindow(Some(hwnd)).as_bool()
                || original.is_some_and(|state| {
                    state.process_id != window_process_id(hwnd) || saved_style.0.is_null()
                })
            {
                self.forget_window(hwnd_value);
                return Ok(false);
            }
            // A rule can match windows Bald never changed. Do not manufacture
            // frames or visibility state for those windows.
            if original.is_none() && saved_style.0.is_null() {
                return Ok(false);
            }
            if let Some(state) = self.original_windows.lock().unwrap().get_mut(&hwnd_value) {
                state.restoring = true;
            }
            if let Some(state) = original.filter(|state| state.native_region) {
                let region = if let Some(saved) = state.original_region {
                    let copy = CreateRectRgn(0, 0, 0, 0);
                    if copy.0.is_null()
                        || CombineRgn(Some(copy), Some(HRGN(saved as *mut c_void)), None, RGN_COPY)
                            .0
                            == 0
                    {
                        let _ = DeleteObject(copy.into());
                        bail!("copy native game region failed");
                    }
                    Some(copy)
                } else {
                    None
                };
                if SetWindowRgn(hwnd, region, false) == 0 {
                    if let Some(region) = region {
                        let _ = DeleteObject(region.into());
                    }
                    bail!("restore native game region failed");
                }
                refresh_native_region(hwnd, state.process_id, None)?;
                if let Some(rendering) = state.native_rendering {
                    let policy = if rendering {
                        DWMNCRP_ENABLED
                    } else {
                        DWMNCRP_DISABLED
                    };
                    DwmSetWindowAttribute(
                        hwnd,
                        DWMWA_NCRENDERING_POLICY,
                        &policy as *const _ as *const c_void,
                        std::mem::size_of_val(&policy) as u32,
                    )?;
                    refresh_native_frame(hwnd)?;
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
                    while native_frame_rendering(hwnd) != Some(rendering) {
                        if !IsWindow(Some(hwnd)).as_bool()
                            || window_process_id(hwnd) != state.process_id
                        {
                            bail!("window changed during native rendering restore");
                        }
                        if std::time::Instant::now() >= deadline {
                            bail!(
                                "native rendering restoration unconfirmed; managed state retained for retry"
                            );
                        }
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                }
                let _ = RedrawWindow(
                    Some(hwnd),
                    None,
                    None,
                    RDW_INVALIDATE | RDW_FRAME | RDW_ALLCHILDREN,
                );
                self.forget_window(hwnd_value);
                let _ = RemovePropW(hwnd, w!("Bald.OriginalStyle"));
                let _ = RemovePropW(hwnd, w!("Bald.NativeFrameRegion"));
                return Ok(true);
            }
            if original.is_some_and(|state| state.clipped)
                || !GetPropW(hwnd, w!("Bald.ClippedFrame")).0.is_null()
            {
                let mut before_client = RECT::default();
                let mut before_origin = POINT::default();
                GetClientRect(hwnd, &mut before_client)?;
                ClientToScreen(hwnd, &mut before_origin).ok()?;
                // Transfer a copy, retaining our saved region until the entire
                // restoration succeeds. A failed frame refresh must be retryable.
                let region = if let Some(saved) = original.and_then(|state| state.original_region) {
                    let copy = CreateRectRgn(0, 0, 0, 0);
                    if copy.0.is_null()
                        || CombineRgn(Some(copy), Some(HRGN(saved as *mut c_void)), None, RGN_COPY)
                            .0
                            == 0
                    {
                        let _ = DeleteObject(copy.into());
                        bail!("copy original game frame region failed");
                    }
                    Some(copy)
                } else {
                    None
                };
                if SetWindowRgn(hwnd, region, false) == 0 {
                    if let Some(region) = region {
                        let _ = DeleteObject(region.into());
                    }
                    bail!("restore game frame region failed");
                }
                // Clearing a region alone leaves Maple's non-client cache in
                // borderless mode. Reapply the saved style to notify the owner,
                // preserving its current visible/minimized/maximized state.
                let current_style = GetWindowLongW(hwnd, GWL_STYLE);
                let saved = original
                    .map(|state| state.style)
                    .unwrap_or(saved_style.0 as isize as i32);
                let live_mask = 0x3100_0000_u32;
                let restored_style =
                    ((saved as u32 & !live_mask) | (current_style as u32 & live_mask)) as i32;
                SetLastError(WIN32_ERROR(0));
                if SetWindowLongW(hwnd, GWL_STYLE, restored_style) == 0 {
                    let error = GetLastError();
                    if error.0 != 0 {
                        bail!("restore game style failed: {}", error.0);
                    }
                }
                if IsIconic(hwnd).as_bool() {
                    if let Some(state) = original {
                        let mut placement = WINDOWPLACEMENT {
                            length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
                            ..Default::default()
                        };
                        GetWindowPlacement(hwnd, &mut placement)?;
                        let normal = placement.rcNormalPosition;
                        if state.clipped_content_size
                            == Some((normal.right - normal.left, normal.bottom - normal.top))
                        {
                            // Keep minimized/show flags intact while restoring the
                            // saved *normal* content geometry. No SC_RESTORE.
                            let placement = restore_normal_placement(placement, state.frame_insets);
                            SetWindowPlacement(hwnd, &placement)?;
                        }
                    }
                }
                // Recalculate after the restored region is installed, rather
                // than measuring the stale client rectangle from SetWindowRgn's
                // in-flight non-client calculation.
                SetWindowPos(
                    hwnd,
                    None,
                    0,
                    0,
                    0,
                    0,
                    SWP_FRAMECHANGED
                        | SWP_NOMOVE
                        | SWP_NOSIZE
                        | SWP_NOACTIVATE
                        | SWP_NOZORDER
                        | SWP_NOREDRAW,
                )?;
                if restored_style as u32 & WS_CAPTION.0 == WS_CAPTION.0 {
                    let mut frame = RECT::default();
                    let mut origin = POINT::default();
                    GetWindowRect(hwnd, &mut frame)?;
                    ClientToScreen(hwnd, &mut origin).ok()?;
                    if IsIconic(hwnd).as_bool() || origin.y <= frame.top {
                        // Re-setting an identical style does not reset every
                        // game's cached borderless mode. Generate real caption
                        // style transitions, without moving/focusing/hiding it.
                        refresh_cached_caption(hwnd, restored_style)?;
                    }
                }
                if !IsIconic(hwnd).as_bool() {
                    let mut frame = RECT::default();
                    let mut origin = POINT::default();
                    GetWindowRect(hwnd, &mut frame)?;
                    ClientToScreen(hwnd, &mut origin).ok()?;
                    if origin.y <= frame.top
                        && let Some(state) = original
                        && state.frame_insets.top > 0
                    {
                        let insets = state.frame_insets;
                        // The clipped outer rectangle has the render dimensions.
                        // Explicitly restore its measured native frame dimensions;
                        // NOMOVE/NOSIZE cannot do this while the owner caches a
                        // full-window client area.
                        SetWindowPos(
                            hwnd,
                            None,
                            before_origin.x - insets.left,
                            before_origin.y - insets.top,
                            before_client.right + insets.left + insets.right,
                            before_client.bottom + insets.top + insets.bottom,
                            SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOREDRAW,
                        )?;
                    }
                }
                // Restoring an old region can itself recalculate a game's client
                // area. Preserve the current render size and origin, including
                // resolutions selected after Bald first managed the window.
                if !IsIconic(hwnd).as_bool() && before_client.right > 0 && before_client.bottom > 0
                {
                    let mut after_client = RECT::default();
                    let mut after_origin = POINT::default();
                    let mut after_window = RECT::default();
                    GetClientRect(hwnd, &mut after_client)?;
                    GetWindowRect(hwnd, &mut after_window)?;
                    ClientToScreen(hwnd, &mut after_origin).ok()?;
                    // Do not treat a still-full-window client area as restored
                    // frame geometry. Doing so immediately shrinks the requested
                    // native outer size back before caption verification.
                    let frame_ready = restored_style as u32 & WS_CAPTION.0 != WS_CAPTION.0
                        || after_origin.y > after_window.top;
                    if frame_ready
                        && (before_client.right != after_client.right
                            || before_client.bottom != after_client.bottom
                            || before_origin.x != after_origin.x
                            || before_origin.y != after_origin.y)
                    {
                        let left = after_origin.x - after_window.left;
                        let top = after_origin.y - after_window.top;
                        SetWindowPos(
                            hwnd,
                            None,
                            before_origin.x - left,
                            before_origin.y - top,
                            before_client.right + after_window.right
                                - after_window.left
                                - after_client.right,
                            before_client.bottom + after_window.bottom
                                - after_window.top
                                - after_client.bottom,
                            SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOREDRAW,
                        )?;
                    }
                }
                if !IsIconic(hwnd).as_bool() && restored_style as u32 & WS_CAPTION.0 == WS_CAPTION.0
                {
                    let mut restored_window = RECT::default();
                    let mut restored_origin = POINT::default();
                    GetWindowRect(hwnd, &mut restored_window)?;
                    ClientToScreen(hwnd, &mut restored_origin).ok()?;
                    if restored_origin.y <= restored_window.top {
                        // A failed frame request must not leave the render area
                        // expanded by caption dimensions. Keep the retry state.
                        SetWindowPos(
                            hwnd,
                            None,
                            before_origin.x,
                            before_origin.y,
                            before_client.right,
                            before_client.bottom,
                            SWP_NOACTIVATE | SWP_NOZORDER,
                        )?;
                        let _ = RedrawWindow(
                            Some(hwnd),
                            None,
                            None,
                            RDW_INVALIDATE | RDW_FRAME | RDW_ALLCHILDREN,
                        );
                        bail!(
                            "game caption restoration incomplete; managed state retained for retry"
                        );
                    }
                }
                let _ = RedrawWindow(
                    Some(hwnd),
                    None,
                    None,
                    RDW_INVALIDATE | RDW_FRAME | RDW_ALLCHILDREN,
                );
                self.forget_window(hwnd_value);
                let _ = RemovePropW(hwnd, w!("Bald.OriginalStyle"));
                let _ = RemovePropW(hwnd, w!("Bald.ClippedFrame"));
                return Ok(true);
            }
            let current = GetWindowLongW(hwnd, GWL_STYLE);
            let mut client = RECT::default();
            let mut client_origin = POINT::default();
            let _ = GetClientRect(hwnd, &mut client);
            let _ = ClientToScreen(hwnd, &mut client_origin);
            let mut current_placement = WINDOWPLACEMENT {
                length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
                ..Default::default()
            };
            GetWindowPlacement(hwnd, &mut current_placement)?;
            let style = original
                .map(|state| state.style)
                .or_else(|| (!saved_style.0.is_null()).then_some(saved_style.0 as isize as i32))
                .unwrap_or_else(|| {
                    (current as u32 | WS_CAPTION.0 | WS_THICKFRAME.0 | WS_SYSMENU.0) as i32
                });
            // Visibility/minimize/maximize bits belong to the live window state,
            // not the snapshot taken when borders were first removed.
            let live_state_mask = 0x3100_0000_u32;
            let style =
                ((style as u32 & !live_state_mask) | (current as u32 & live_state_mask)) as i32;
            if let Some(original) = original {
                let current_ex = GetWindowLongW(hwnd, GWL_EXSTYLE);
                // Restore only the frame bits we changed. Preserve unrelated
                // live taskbar/topmost/layered flags owned by the application.
                let restored_ex = ((current_ex as u32 & !BORDER_EX_STYLES)
                    | (original.ex_style as u32 & BORDER_EX_STYLES))
                    as i32;
                set_window_long_checked(hwnd, GWL_EXSTYLE, restored_ex)?;
            }
            SetLastError(WIN32_ERROR(0));
            let previous = SetWindowLongW(hwnd, GWL_STYLE, style);
            if previous == 0 {
                let error = GetLastError();
                if error.0 != 0 {
                    bail!("restore SetWindowLongW failed: {}", error.0);
                }
            }
            // Let the owner refresh its frame state, without permitting a move
            // or resize. NOSENDCHANGING is for direct geometry writes, not this
            // notification: some owners otherwise retain a full-window client.
            refresh_native_frame(hwnd)?;
            if original.is_some_and(|state| state.native_rendering == Some(true))
                && native_frame_rendering(hwnd) == Some(false)
            {
                let policy = DWMNCRP_ENABLED;
                DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_NCRENDERING_POLICY,
                    &policy as *const _ as *const c_void,
                    std::mem::size_of_val(&policy) as u32,
                )?;
                refresh_native_frame(hwnd)?;
            }
            let frame_was_deferred = if let Some(original) = original {
                wait_for_native_frame(hwnd, original, current as u32 & WS_MINIMIZE.0 != 0)?
            } else {
                false
            };
            let mut restore_origin = client_origin;
            if let Some(original) = original {
                let insets = original.frame_insets;
                if IsIconic(hwnd).as_bool() && current as u32 & BORDER_STYLES == 0 {
                    // Preserve showCmd/flags and normal (workspace) coordinates.
                    // Never use the minimized -32000 screen rectangle.
                    let placement = restore_normal_placement(current_placement, insets);
                    SetWindowPlacement(hwnd, &placement)?;
                } else if !IsIconic(hwnd).as_bool()
                    && !IsZoomed(hwnd).as_bool()
                    && client.right > 0
                    && client.bottom > 0
                {
                    let mut frame = RECT::default();
                    let mut actual_client = RECT::default();
                    let mut origin = POINT::default();
                    GetWindowRect(hwnd, &mut frame)?;
                    GetClientRect(hwnd, &mut actual_client)?;
                    ClientToScreen(hwnd, &mut origin).ok()?;
                    if insets.top > 0 && origin.y <= frame.top {
                        bail!(
                            "native caption restoration incomplete; managed state retained for retry"
                        );
                    }
                    // Measure the restored frame, including DWM differences.
                    // Never grow content by a cached/classic caption's height.
                    if frame_was_deferred {
                        // The user may have moved the window while its owner
                        // refreshed the frame. Do not jump back to stale coordinates.
                        restore_origin = origin;
                    }
                    SetWindowPos(
                        hwnd,
                        None,
                        restore_origin.x - (origin.x - frame.left),
                        restore_origin.y - (origin.y - frame.top),
                        client.right + frame.right - frame.left - actual_client.right,
                        client.bottom + frame.bottom - frame.top - actual_client.bottom,
                        QUIET_POSITION | owner_frame_dispatch(hwnd),
                    )?;
                    if is_native_region_game(hwnd) {
                        wait_for_frame_geometry(
                            hwnd,
                            restore_origin.x - (origin.x - frame.left),
                            restore_origin.y - (origin.y - frame.top),
                            client.right + frame.right - frame.left - actual_client.right,
                            client.bottom + frame.bottom - frame.top - actual_client.bottom,
                        )?;
                    }
                }
            }
            if !IsIconic(hwnd).as_bool() && original.is_some_and(|state| state.frame_insets.top > 0)
            {
                let mut frame = RECT::default();
                let mut origin = POINT::default();
                GetWindowRect(hwnd, &mut frame)?;
                ClientToScreen(hwnd, &mut origin).ok()?;
                if origin.y <= frame.top {
                    // A style bit alone is not proof that the caption returned.
                    // Preserve current content geometry and retry ownership.
                    SetWindowPos(
                        hwnd,
                        None,
                        restore_origin.x,
                        restore_origin.y,
                        client.right,
                        client.bottom,
                        QUIET_POSITION,
                    )?;
                    bail!(
                        "native caption restoration incomplete; managed state retained for retry"
                    );
                }
            }
            self.original_windows.lock().unwrap().remove(&hwnd_value);
            let _ = RemovePropW(hwnd, w!("Bald.OriginalStyle"));
            let _ = RemovePropW(hwnd, w!("Bald.OriginalExStyle"));
            let _ = RedrawWindow(Some(hwnd), None, None, RDW_INVALIDATE | RDW_FRAME);
        }
        Ok(true)
    }

    pub fn begin_shutdown(&self) -> bool {
        !self.shutting_down.swap(true, Ordering::AcqRel)
    }

    fn forget_window(&self, hwnd_value: isize) {
        if let Some(state) = self.original_windows.lock().unwrap().remove(&hwnd_value) {
            if let Some(region) = state.original_region {
                unsafe {
                    let _ = DeleteObject(HRGN(region as *mut c_void).into());
                }
            }
        }
    }

    pub fn managed_windows(&self) -> Vec<WindowInfo> {
        let handles: Vec<_> = self
            .original_windows
            .lock()
            .unwrap()
            .keys()
            .copied()
            .collect();
        let mut windows = Vec::new();
        for value in handles {
            let hwnd = HWND(value as *mut c_void);
            let valid = unsafe {
                IsWindow(Some(hwnd)).as_bool()
                    && !GetPropW(hwnd, w!("Bald.OriginalStyle")).0.is_null()
            } && self
                .original_windows
                .lock()
                .unwrap()
                .get(&value)
                .is_some_and(|state| state.process_id == window_process_id(hwnd));
            if valid {
                windows.push(describe_window(hwnd));
            } else {
                self.forget_window(value);
            }
        }
        windows
    }

    #[cfg(test)]
    pub fn has_drag_targets(&self) -> bool {
        let handles: Vec<_> = self.drag_rules.lock().unwrap().keys().copied().collect();
        handles
            .into_iter()
            .any(|hwnd| self.drag_context(hwnd).is_some())
    }

    pub fn has_strip_targets(&self) -> bool {
        let handles: Vec<_> = self.drag_rules.lock().unwrap().keys().copied().collect();
        handles
            .into_iter()
            .any(|hwnd| self.strip_context(hwnd).is_some())
    }

    fn drag_context(&self, hwnd_value: isize) -> Option<(u32, i32)> {
        if !self.drag_allowed(hwnd_value) {
            return None;
        }
        self.strip_context(hwnd_value)
    }

    fn strip_context(&self, hwnd_value: isize) -> Option<(u32, i32)> {
        if self.shutting_down.load(Ordering::Acquire) {
            return None;
        }
        let process_id = self
            .drag_rules
            .try_lock()
            .ok()?
            .get(&hwnd_value)?
            .process_id;
        let state = self
            .original_windows
            .try_lock()
            .ok()?
            .get(&hwnd_value)
            .copied();
        if state.is_some_and(|state| {
            state.restoring
                || (state.clipped && !state.native_region)
                || state.process_id != process_id
        }) {
            return None;
        }
        let hwnd = HWND(hwnd_value as *mut c_void);
        if unsafe {
            !IsWindow(Some(hwnd)).as_bool()
                || !IsWindowVisible(hwnd).as_bool()
                || IsIconic(hwnd).as_bool()
                || IsZoomed(hwnd).as_bool()
                || window_process_id(hwnd) != process_id
                || (GetWindowLongW(hwnd, GWL_STYLE) as u32 & BORDER_STYLES != 0
                    && !state.is_some_and(|state| state.native_region))
        } {
            return None;
        }
        // An already-borderless registered window can be explicitly allowed
        // without inventing an original frame snapshot or taking restore ownership.
        Some((process_id, DRAG_HEIGHT))
    }

    pub fn restore_all_borders(&self) -> Result<()> {
        let handles: Vec<_> = self
            .original_windows
            .lock()
            .unwrap()
            .keys()
            .copied()
            .collect();
        self.restore_handles(handles)
    }

    pub fn restore_handles(&self, handles: Vec<isize>) -> Result<()> {
        let mut errors = Vec::new();
        for hwnd in handles {
            if let Err(error) = self.restore_borders(hwnd) {
                errors.push(format!("{hwnd:x}: {error}"));
            }
        }
        if !errors.is_empty() {
            bail!("frame restoration failed: {}", errors.join("; "));
        }
        Ok(())
    }

    pub fn native_drag_target(&self, cursor: POINT) -> Option<isize> {
        if self.shutting_down.load(Ordering::Acquire) {
            return None;
        }

        // Do not send hit-test messages into a game's thread from the low-level
        // mouse hook. Only an already-active, approved window can start a drag.
        let root = unsafe { GetForegroundWindow() };
        if root.0.is_null() {
            return None;
        }
        let hwnd_value = root.0 as isize;
        if !self.can_begin_drag(hwnd_value) {
            return None;
        }
        let (_, drag_height) = self.drag_context(hwnd_value)?;
        let rect = self.drag_surface(root)?;
        if !is_drag_point(cursor, rect, drag_height) {
            return None;
        }
        Some(hwnd_value)
    }

    pub fn native_blocked_drag_target(&self, cursor: POINT) -> Option<isize> {
        let hwnd = unsafe { GetForegroundWindow() };
        self.blocked_strip_target(hwnd.0 as isize, cursor)
    }

    fn blocked_strip_target(&self, hwnd_value: isize, cursor: POINT) -> Option<isize> {
        if !self
            .drag_rules
            .try_lock()
            .ok()?
            .get(&hwnd_value)
            .is_some_and(|rule| rule.mode == DragMode::Disabled)
        {
            return None;
        }
        let (_, height) = self.strip_context(hwnd_value)?;
        let rect = self.drag_surface(HWND(hwnd_value as *mut c_void))?;
        is_drag_point(cursor, rect, height).then_some(hwnd_value)
    }

    fn drag_surface(&self, hwnd: HWND) -> Option<RECT> {
        let native = self
            .original_windows
            .try_lock()
            .ok()?
            .get(&(hwnd.0 as isize))
            .is_some_and(|state| state.native_region);
        let mut rect = RECT::default();
        unsafe {
            if native {
                GetClientRect(hwnd, &mut rect).ok()?;
                let mut origin = POINT::default();
                ClientToScreen(hwnd, &mut origin).ok().ok()?;
                rect.left += origin.x;
                rect.right += origin.x;
                rect.top += origin.y;
                rect.bottom += origin.y;
            } else {
                GetWindowRect(hwnd, &mut rect).ok()?;
            }
        }
        Some(rect)
    }

    pub(crate) fn begin_user_drag(&self, hwnd_value: isize, cursor: POINT) -> Option<UserDrag> {
        if self.shutting_down.load(Ordering::Acquire) || !self.can_begin_drag(hwnd_value) {
            return None;
        }
        let (process_id, _) = self.drag_context(hwnd_value)?;
        let hwnd = HWND(hwnd_value as *mut c_void);
        let mut rect = RECT::default();
        unsafe {
            if GetWindowRect(hwnd, &mut rect).is_err() {
                return None;
            }
        }
        Some(UserDrag {
            hwnd: hwnd_value,
            process_id,
            window_origin: POINT {
                x: rect.left,
                y: rect.top,
            },
            cursor_origin: cursor,
            pending: None,
            position_flags: user_drag_position_flags(hwnd),
        })
    }

    pub(crate) fn poll_user_drag(&self, drag: &mut UserDrag) -> Result<bool> {
        let permitted = self.drag_allowed(drag.hwnd);
        let Some(pending) = drag.pending else {
            return Ok(permitted);
        };
        let hwnd = HWND(drag.hwnd as *mut c_void);
        let mut actual = RECT::default();
        unsafe {
            if !IsWindow(Some(hwnd)).as_bool()
                || window_process_id(hwnd) != drag.process_id
                || IsIconic(hwnd).as_bool()
                || !IsWindowVisible(hwnd).as_bool()
            {
                return Ok(false);
            }
            GetWindowRect(hwnd, &mut actual)?;
        }
        if (actual.left, actual.top) == (pending.x, pending.y) {
            drag.pending = None;
            if let Some(rule) = self.drag_rules.lock().unwrap().get_mut(&drag.hwnd) {
                rule.in_flight = false;
            }
        } else if pending.started.elapsed() >= std::time::Duration::from_millis(100) {
            if let Some(rule) = self
                .drag_rules
                .lock()
                .unwrap()
                .get_mut(&drag.hwnd)
                .filter(|rule| rule.process_id == drag.process_id)
            {
                rule.stalled = true;
                rule.in_flight = false;
            }
            return Ok(false);
        }
        Ok(permitted || drag.pending.is_some())
    }

    pub(crate) fn drag_move_pending(drag: &UserDrag) -> bool {
        drag.pending.is_some()
    }

    pub(crate) fn move_user_drag(&self, drag: &mut UserDrag, cursor: POINT) -> Result<bool> {
        let Ok(_operation) = self.operations.try_lock() else {
            return Ok(true);
        };
        let hwnd = HWND(drag.hwnd as *mut c_void);
        if !self
            .drag_context(drag.hwnd)
            .is_some_and(|(pid, _)| pid == drag.process_id)
        {
            return if drag.pending.is_some() {
                self.poll_user_drag(drag)
            } else {
                Ok(false)
            };
        }
        if !self.poll_user_drag(drag)? {
            return Ok(false);
        }
        if drag.pending.is_some() {
            return Ok(true);
        }
        let x = drag
            .window_origin
            .x
            .saturating_add(cursor.x.saturating_sub(drag.cursor_origin.x));
        let y = drag
            .window_origin
            .y
            .saturating_add(cursor.y.saturating_sub(drag.cursor_origin.y));
        unsafe {
            let mut actual = RECT::default();
            GetWindowRect(hwnd, &mut actual)?;
            if (actual.left, actual.top) == (x, y) {
                return Ok(true);
            }
            if let Some(rule) = self.drag_rules.lock().unwrap().get_mut(&drag.hwnd) {
                rule.in_flight = true;
            }
            let moved = SetWindowPos(hwnd, None, x, y, 0, 0, drag.position_flags);
            if let Err(error) = moved {
                if let Some(rule) = self.drag_rules.lock().unwrap().get_mut(&drag.hwnd) {
                    rule.in_flight = false;
                    rule.stalled = true;
                }
                return Err(error.into());
            }
        }
        drag.pending = Some(PendingDragMove {
            x,
            y,
            started: std::time::Instant::now(),
        });
        Ok(true)
    }
}

fn is_native_region_game(hwnd: HWND) -> bool {
    let mut class = [0_u16; 256];
    let length = unsafe { GetClassNameW(hwnd, &mut class) };
    String::from_utf16_lossy(&class[..length.max(0) as usize]) == "MapleStoryClass"
}

fn refresh_native_region(hwnd: HWND, process_id: u32, content: Option<(i32, i32)>) -> Result<()> {
    unsafe {
        SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_ASYNCWINDOWPOS | SWP_NOMOVE | SWP_NOSIZE | QUIET_POSITION,
        )?;
        let started = std::time::Instant::now();
        loop {
            if !IsWindow(Some(hwnd)).as_bool() || window_process_id(hwnd) != process_id {
                bail!("game window changed during native frame refresh");
            }
            // Region restoration must not unminimize or show a hidden game.
            if IsIconic(hwnd).as_bool() || !IsWindowVisible(hwnd).as_bool() {
                return Ok(());
            }
            let mut outer = RECT::default();
            let mut client = RECT::default();
            let mut origin = POINT::default();
            GetWindowRect(hwnd, &mut outer)?;
            GetClientRect(hwnd, &mut client)?;
            ClientToScreen(hwnd, &mut origin).ok()?;
            if origin.y > outer.top
                && content.is_none_or(|size| size == (client.right, client.bottom))
            {
                return Ok(());
            }
            if started.elapsed() >= std::time::Duration::from_secs(1) {
                bail!(
                    "native game frame refresh was not acknowledged; managed state retained for retry"
                );
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

fn user_drag_position_flags(hwnd: HWND) -> SET_WINDOW_POS_FLAGS {
    let flags = SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_NOSIZE | SWP_ASYNCWINDOWPOS;
    let mut class = [0u16; 256];
    let length = unsafe { GetClassNameW(hwnd, &mut class) };
    // Maple's frame-less move must retain the earlier notification bypass.
    // Other owners keep their position constraints, including fixed-center games.
    if String::from_utf16_lossy(&class[..length.max(0) as usize]) == "MapleStoryClass" {
        flags | SWP_NOSENDCHANGING
    } else {
        flags
    }
}

fn process_is_elevated(process: HANDLE) -> Option<bool> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(process, TOKEN_QUERY, &mut token).ok()?;
        let mut elevation = TOKEN_ELEVATION::default();
        let mut returned = 0;
        let result = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut c_void),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        );
        let _ = CloseHandle(token);
        result.ok()?;
        Some(elevation.TokenIsElevated != 0)
    }
}

fn set_style_checked(hwnd: HWND, style: i32) -> Result<()> {
    set_window_long_checked(hwnd, GWL_STYLE, style)
}

fn native_frame_rendering(hwnd: HWND) -> Option<bool> {
    let mut enabled = 0_i32;
    unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_NCRENDERING_ENABLED,
            &mut enabled as *mut _ as *mut c_void,
            std::mem::size_of_val(&enabled) as u32,
        )
        .ok()?;
    }
    Some(enabled != 0)
}

fn owner_frame_dispatch(hwnd: HWND) -> SET_WINDOW_POS_FLAGS {
    if is_native_region_game(hwnd) {
        SWP_ASYNCWINDOWPOS
    } else {
        SET_WINDOW_POS_FLAGS(0)
    }
}

fn wait_for_frame_geometry(hwnd: HWND, x: i32, y: i32, width: i32, height: i32) -> Result<()> {
    let process_id = window_process_id(hwnd);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    loop {
        unsafe {
            if !IsWindow(Some(hwnd)).as_bool() || window_process_id(hwnd) != process_id {
                bail!("window changed during owner frame update");
            }
            if IsIconic(hwnd).as_bool() || !IsWindowVisible(hwnd).as_bool() {
                bail!("window became hidden or minimized during owner frame update");
            }
            let mut frame = RECT::default();
            GetWindowRect(hwnd, &mut frame)?;
            if (
                frame.left,
                frame.top,
                frame.right - frame.left,
                frame.bottom - frame.top,
            ) == (x, y, width, height)
            {
                return Ok(());
            }
        }
        if std::time::Instant::now() >= deadline {
            bail!("owner frame geometry unconfirmed; managed state retained for retry");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn refresh_native_frame(hwnd: HWND) -> Result<()> {
    unsafe {
        SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED
                | SWP_NOMOVE
                | SWP_NOSIZE
                | SWP_NOACTIVATE
                | SWP_NOZORDER
                | SWP_NOOWNERZORDER
                | owner_frame_dispatch(hwnd),
        )?;
    }
    Ok(())
}

fn wait_for_native_frame(
    hwnd: HWND,
    original: OriginalWindowState,
    originally_minimized: bool,
) -> Result<bool> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut waiting = false;
    loop {
        unsafe {
            // The owner can finish frame recalculation after SetWindowPos returns.
            // Observe only: do not resend style/size changes while it settles.
            if !IsWindow(Some(hwnd)).as_bool() || window_process_id(hwnd) != original.process_id {
                bail!("window changed while confirming native frame restoration");
            }
            let mut frame = RECT::default();
            let mut origin = POINT::default();
            GetWindowRect(hwnd, &mut frame)?;
            ClientToScreen(hwnd, &mut origin).ok()?;
            let minimized = IsIconic(hwnd).as_bool();
            if minimized && !originally_minimized && original.frame_insets.top > 0 {
                // A minimized zero-client rectangle cannot prove the visible
                // caption returned. Keep ownership for a later confirmation.
                bail!(
                    "caption confirmation deferred while minimized; managed state retained for retry"
                );
            }
            let caption_ready = original.frame_insets.top <= 0
                || (minimized && originally_minimized)
                || (!minimized && origin.y > frame.top);
            let rendering_ready = original.native_rendering != Some(true)
                || native_frame_rendering(hwnd) == Some(true);
            if caption_ready && rendering_ready {
                return Ok(waiting);
            }
            if std::time::Instant::now() >= deadline {
                bail!(
                    "native frame restoration unconfirmed after 5s; managed state retained for retry"
                );
            }
            waiting = true;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

fn set_window_long_checked(
    hwnd: HWND,
    index: windows::Win32::UI::WindowsAndMessaging::WINDOW_LONG_PTR_INDEX,
    value: i32,
) -> Result<()> {
    unsafe {
        SetLastError(WIN32_ERROR(0));
        let previous = SetWindowLongW(hwnd, index, value);
        let error = GetLastError();
        if previous == 0 {
            if error.0 != 0 {
                bail!("set window style failed: {}", error.0);
            }
        }
    }
    Ok(())
}

fn refresh_cached_caption(hwnd: HWND, restored_style: i32) -> Result<()> {
    let flags =
        SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOREDRAW;
    set_style_checked(hwnd, (restored_style as u32 & !WS_CAPTION.0) as i32)?;
    let first_refresh = unsafe { SetWindowPos(hwnd, None, 0, 0, 0, 0, flags) };
    // Always put the original style back, even when the intermediate refresh fails.
    set_style_checked(hwnd, restored_style)?;
    unsafe {
        SetWindowPos(hwnd, None, 0, 0, 0, 0, flags)?;
    }
    first_refresh?;
    Ok(())
}

fn requires_elevation(hwnd: HWND) -> bool {
    unsafe {
        if process_is_elevated(GetCurrentProcess()) != Some(false) {
            return false;
        }
        let Ok(process) = OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION,
            false,
            window_process_id(hwnd),
        ) else {
            return false;
        };
        let elevated = process_is_elevated(process);
        let _ = CloseHandle(process);
        elevated == Some(true)
    }
}

pub fn is_borderless(hwnd_value: isize) -> bool {
    let hwnd = HWND(hwnd_value as *mut c_void);
    unsafe {
        IsWindow(Some(hwnd)).as_bool()
            && (GetWindowLongW(hwnd, GWL_STYLE) as u32 & BORDER_STYLES == 0
                || !GetPropW(hwnd, w!("Bald.ClippedFrame")).0.is_null()
                || !GetPropW(hwnd, w!("Bald.NativeFrameRegion")).0.is_null())
    }
}

fn is_drag_point(point: POINT, rect: RECT, drag_height: i32) -> bool {
    point.x >= rect.left
        && point.x < rect.right
        && point.y >= rect.top
        && point.y < rect.top + drag_height
}

fn window_process_id(hwnd: HWND) -> u32 {
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
    }
    pid
}

fn restore_normal_placement(mut placement: WINDOWPLACEMENT, insets: RECT) -> WINDOWPLACEMENT {
    placement.rcNormalPosition.left -= insets.left;
    placement.rcNormalPosition.top -= insets.top;
    placement.rcNormalPosition.right += insets.right;
    placement.rcNormalPosition.bottom += insets.bottom;
    placement
}

fn describe_window(hwnd: HWND) -> WindowInfo {
    unsafe {
        let mut title = [0u16; 512];
        let title_len = GetWindowTextW(hwnd, &mut title).max(0) as usize;
        let path = process_path(window_process_id(hwnd));
        let executable_name = path
            .as_ref()
            .and_then(|path| std::path::Path::new(path).file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("Unknown")
            .to_owned();
        let mut class = [0u16; 256];
        let class_len = GetClassNameW(hwnd, &mut class).max(0) as usize;
        WindowInfo {
            hwnd: hwnd.0 as isize,
            title: String::from_utf16_lossy(&title[..title_len]),
            executable_name,
            executable_path: path,
            class_name: String::from_utf16_lossy(&class[..class_len]),
            is_borderless: GetWindowLongW(hwnd, GWL_STYLE) as u32 & BORDER_STYLES == 0
                || !GetPropW(hwnd, w!("Bald.ClippedFrame")).0.is_null()
                || !GetPropW(hwnd, w!("Bald.NativeFrameRegion")).0.is_null(),
        }
    }
}

static UI_PROCESS_ID: AtomicU32 = AtomicU32::new(0);

pub fn set_ui_process_id(process_id: u32) {
    UI_PROCESS_ID.store(process_id, Ordering::Release);
}

pub fn enumerate_windows() -> Vec<WindowInfo> {
    let mut windows: Vec<WindowInfo> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(enum_window), LPARAM(&mut windows as *mut _ as isize));
    }
    windows.sort_by_key(|window| window.title.to_lowercase());
    windows
}

unsafe extern "system" fn enum_window(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
    unsafe {
        // GetWindowText sends WM_GETTEXT to same-process windows. The UI may be
        // waiting for this scan's config guard, so skip ourselves before any query.
        let process_id = window_process_id(hwnd);
        if process_id == std::process::id() || process_id == UI_PROCESS_ID.load(Ordering::Acquire) {
            return true.into();
        }
        if (!IsWindowVisible(hwnd).as_bool()
            && GetPropW(hwnd, w!("Bald.OriginalStyle")).0.is_null())
            || GetWindow(hwnd, GW_OWNER)
                .ok()
                .is_some_and(|owner| !owner.0.is_null())
            || GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0
        {
            return true.into();
        }
        let mut title = [0u16; 512];
        let title_len = GetWindowTextW(hwnd, &mut title);
        if title_len <= 0 {
            return true.into();
        }
        let title = String::from_utf16_lossy(&title[..title_len as usize]);
        if title.trim().is_empty() || title == "Program Manager" || title == "Bald" {
            return true.into();
        }
        let info = describe_window(hwnd);
        if info.executable_name.eq_ignore_ascii_case("bald.exe") {
            return true.into();
        }
        let output = &mut *(lparam.0 as *mut Vec<WindowInfo>);
        output.push(info);
        true.into()
    }
}

fn process_path(pid: u32) -> Option<String> {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = vec![0u16; 32768];
        let mut len = buffer.len() as u32;
        let result = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buffer.as_mut_ptr()),
            &mut len,
        );
        let _ = CloseHandle(process);
        result.ok()?;
        Some(String::from_utf16_lossy(&buffer[..len as usize]))
    }
}

#[cfg(test)]
#[path = "window_manager_windows_tests.rs"]
mod windows_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crops_transparent_icon_canvas() {
        let mut rgba = vec![0; 4 * 4 * 4];
        for y in 1..3 {
            for x in 1..3 {
                rgba[((y * 4 + x) * 4 + 3) as usize] = 255;
            }
        }
        let cropped = crop_transparent_padding(ExecutableIcon {
            width: 4,
            height: 4,
            rgba,
        });
        assert_eq!((cropped.width, cropped.height), (2, 2));
        assert_eq!(cropped.rgba.len(), 2 * 2 * 4);
    }

    #[test]
    fn prefers_more_native_icon_detail_over_padded_canvas_size() {
        let mut rgba = vec![0; 10 * 10 * 4];
        for y in 4..7 {
            for x in 4..7 {
                rgba[(y * 10 + x) * 4 + 3] = 255;
            }
        }
        let icon = ExecutableIcon {
            width: 10,
            height: 10,
            rgba,
        };
        let detailed = ExecutableIcon {
            width: 5,
            height: 5,
            rgba: vec![255; 5 * 5 * 4],
        };
        let chosen = icon_resources::best_native_icon([icon, detailed]).unwrap();
        assert_eq!((chosen.width, chosen.height), (5, 5));
    }

    #[test]
    fn dragging_is_limited_to_the_top_sixteen_physical_pixels() {
        let rect = RECT {
            left: 100,
            top: 200,
            right: 900,
            bottom: 700,
        };
        assert_eq!(DRAG_HEIGHT, 16);
        assert!(is_drag_point(POINT { x: 400, y: 200 }, rect, DRAG_HEIGHT));
        assert!(is_drag_point(POINT { x: 400, y: 215 }, rect, DRAG_HEIGHT));
        assert!(!is_drag_point(POINT { x: 400, y: 216 }, rect, DRAG_HEIGHT));
        assert!(!is_drag_point(POINT { x: 400, y: 199 }, rect, DRAG_HEIGHT));
        assert!(!is_drag_point(POINT { x: 99, y: 200 }, rect, DRAG_HEIGHT));
    }

    #[test]
    fn shutdown_is_only_started_once() {
        let controller = WindowController::default();
        assert!(controller.begin_shutdown());
        assert!(!controller.begin_shutdown());
    }

    #[test]
    fn extracts_a_renderable_executable_icon() {
        let path = std::env::current_exe().unwrap();
        let icon = executable_icon(path.to_str().unwrap()).expect("executable icon");
        assert!(icon.width >= 32);
        assert_eq!(icon.rgba.len(), (icon.width * icon.height * 4) as usize);
    }
}
