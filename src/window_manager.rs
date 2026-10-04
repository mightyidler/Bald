// Window-style behavior is derived from ihateborders (GPL-3.0),
// https://github.com/Z1xus/ihateborders, Copyright its contributors.

use std::{
    collections::HashMap,
    ffi::c_void,
    sync::{Arc, Mutex},
};

use anyhow::{Result, bail};
use windows::{
    Win32::{
        Foundation::{
            CloseHandle, GetLastError, HANDLE, HWND, LPARAM, POINT, RECT, SetLastError, WIN32_ERROR,
        },
        Graphics::Gdi::ClientToScreen,
        System::Threading::{
            OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
            QueryFullProcessImageNameW,
        },
        UI::WindowsAndMessaging::{
            EnumWindows, GA_ROOT, GW_OWNER, GWL_EXSTYLE, GWL_STYLE, GetAncestor, GetClassNameW,
            GetClientRect, GetPropW, GetWindow, GetWindowLongW, GetWindowPlacement, GetWindowRect,
            GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible,
            RemovePropW, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
            SetPropW, SetWindowLongW, SetWindowPlacement, SetWindowPos, WINDOWPLACEMENT, WS_BORDER,
            WS_CAPTION, WS_DLGFRAME, WS_EX_TOOLWINDOW, WS_SYSMENU, WS_THICKFRAME, WindowFromPoint,
        },
    },
    core::w,
};

pub struct ExecutableIcon {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub fn executable_icon(path: &str) -> Option<ExecutableIcon> {
    match high_resolution_executable_icon(path) {
        Some(icon) if visible_pixel_ratio(&icon) >= 0.2 => Some(icon),
        Some(icon) => legacy_executable_icon(path).or(Some(icon)),
        None => legacy_executable_icon(path),
    }
}

fn visible_pixel_ratio(icon: &ExecutableIcon) -> f32 {
    let visible = icon
        .rgba
        .chunks_exact(4)
        .filter(|pixel| pixel[3] > 8)
        .count();
    visible as f32 / (icon.width * icon.height).max(1) as f32
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
            System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx},
            UI::Shell::{
                IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK,
                SIIGBF_ICONONLY,
            },
        },
        core::PCWSTR,
    };

    let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
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
    original_windows: Arc<Mutex<HashMap<isize, OriginalWindowState>>>,
    drag: Arc<Mutex<DragMonitorState>>,
}

#[derive(Debug, Clone, Copy)]
struct OriginalWindowState {
    style: i32,
    placement: WINDOWPLACEMENT,
    drag_height: i32,
}

#[derive(Debug, Default)]
struct DragMonitorState {
    active: Option<ActiveDrag>,
}

#[derive(Debug, Clone, Copy)]
struct ActiveDrag {
    hwnd: isize,
    cursor_origin: POINT,
    window_left: i32,
    window_top: i32,
}

impl WindowController {
    pub fn make_borderless(&self, hwnd_value: isize) -> Result<bool> {
        if self
            .drag
            .lock()
            .unwrap()
            .active
            .is_some_and(|drag| drag.hwnd == hwnd_value)
        {
            return Ok(false);
        }
        let hwnd = HWND(hwnd_value as *mut c_void);
        unsafe {
            if !IsWindow(Some(hwnd)).as_bool() {
                bail!("window no longer exists");
            }
            if IsIconic(hwnd).as_bool() {
                return Ok(false);
            }
            let current = GetWindowLongW(hwnd, GWL_STYLE);
            if current as u32 & BORDER_STYLES == 0 {
                return Ok(false);
            }

            // Keep the existing client area stable in screen coordinates. Some games cache
            // their render/input surface and otherwise end up drawing into the former caption
            // area while still hit-testing against the old client origin.
            let mut window_rect = RECT::default();
            GetWindowRect(hwnd, &mut window_rect)?;
            let mut placement = WINDOWPLACEMENT {
                length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
                ..Default::default()
            };
            GetWindowPlacement(hwnd, &mut placement)?;
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

            self.original_windows
                .lock()
                .unwrap()
                .entry(hwnd_value)
                .or_insert(OriginalWindowState {
                    style: current,
                    placement,
                    drag_height: (client_origin.y - window_rect.top).clamp(24, 48),
                });
            let _ = SetPropW(
                hwnd,
                w!("Bald.OriginalStyle"),
                Some(HANDLE(current as isize as *mut c_void)),
            );
            SetLastError(WIN32_ERROR(0));
            let previous =
                SetWindowLongW(hwnd, GWL_STYLE, (current as u32 & !BORDER_STYLES) as i32);
            if previous == 0 {
                let error = GetLastError();
                if error.0 != 0 {
                    self.original_windows.lock().unwrap().remove(&hwnd_value);
                    if error.0 == 5 {
                        bail!("elevation_required");
                    }
                    bail!("SetWindowLongW failed: {}", error.0);
                }
            }
            SetWindowPos(
                hwnd,
                None,
                client_origin.x,
                client_origin.y,
                client_width,
                client_height,
                SWP_FRAMECHANGED | SWP_NOZORDER | SWP_NOACTIVATE,
            )?;
            Ok(true)
        }
    }

    pub fn restore_borders(&self, hwnd_value: isize) -> Result<bool> {
        let hwnd = HWND(hwnd_value as *mut c_void);
        unsafe {
            if !IsWindow(Some(hwnd)).as_bool() {
                bail!("window no longer exists");
            }
            let original = self
                .original_windows
                .lock()
                .unwrap()
                .get(&hwnd_value)
                .copied();
            let saved_style = GetPropW(hwnd, w!("Bald.OriginalStyle"));
            let current = GetWindowLongW(hwnd, GWL_STYLE);
            let style = original
                .map(|state| state.style)
                .or_else(|| (!saved_style.0.is_null()).then_some(saved_style.0 as isize as i32))
                .unwrap_or_else(|| {
                    (current as u32 | WS_CAPTION.0 | WS_THICKFRAME.0 | WS_SYSMENU.0) as i32
                });
            SetLastError(WIN32_ERROR(0));
            let previous = SetWindowLongW(hwnd, GWL_STYLE, style);
            if previous == 0 {
                let error = GetLastError();
                if error.0 != 0 {
                    bail!("restore SetWindowLongW failed: {}", error.0);
                }
            }
            if let Some(original) = original {
                SetWindowPlacement(hwnd, &original.placement)?;
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
            self.original_windows.lock().unwrap().remove(&hwnd_value);
            let _ = RemovePropW(hwnd, w!("Bald.OriginalStyle"));
        }
        Ok(true)
    }

    pub fn begin_window_drag(&self, cursor: POINT) -> bool {
        let mut drag = self.drag.lock().unwrap();
        if drag.active.is_some() {
            return false;
        }

        let hovered = unsafe { WindowFromPoint(cursor) };
        let root = unsafe { GetAncestor(hovered, GA_ROOT) };
        if root.0.is_null() {
            return false;
        }
        let hwnd_value = root.0 as isize;
        let drag_height = self
            .original_windows
            .lock()
            .unwrap()
            .get(&hwnd_value)
            .map(|state| state.drag_height);
        let Some(drag_height) = drag_height else {
            return false;
        };
        let mut rect = RECT::default();
        if unsafe { GetWindowRect(root, &mut rect) }.is_err()
            || !is_drag_point(cursor, rect, drag_height)
        {
            return false;
        }

        drag.active = Some(ActiveDrag {
            hwnd: hwnd_value,
            cursor_origin: cursor,
            window_left: rect.left,
            window_top: rect.top,
        });
        true
    }

    pub fn move_window_drag(&self, cursor: POINT) -> bool {
        let active = self.drag.lock().unwrap().active;
        let Some(active) = active else {
            return false;
        };
        let (x, y) = drag_position(active, cursor);
        let hwnd = HWND(active.hwnd as *mut c_void);
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                None,
                x,
                y,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        if let Some(original) = self.original_windows.lock().unwrap().get_mut(&active.hwnd) {
            let width = original.placement.rcNormalPosition.right
                - original.placement.rcNormalPosition.left;
            let height = original.placement.rcNormalPosition.bottom
                - original.placement.rcNormalPosition.top;
            original.placement.rcNormalPosition = RECT {
                left: x,
                top: y,
                right: x + width,
                bottom: y + height,
            };
        }
        true
    }

    pub fn end_window_drag(&self) -> bool {
        self.drag.lock().unwrap().active.take().is_some()
    }
}

fn is_drag_point(point: POINT, rect: RECT, drag_height: i32) -> bool {
    point.x >= rect.left
        && point.x < rect.right
        && point.y >= rect.top
        && point.y < rect.top + drag_height
}

fn drag_position(drag: ActiveDrag, cursor: POINT) -> (i32, i32) {
    (
        drag.window_left + cursor.x - drag.cursor_origin.x,
        drag.window_top + cursor.y - drag.cursor_origin.y,
    )
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
        if !IsWindowVisible(hwnd).as_bool()
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
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let path = process_path(pid);
        let executable_name = path
            .as_ref()
            .and_then(|path| std::path::Path::new(path).file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("Unknown")
            .to_owned();
        if executable_name.eq_ignore_ascii_case("bald.exe") {
            return true.into();
        }
        let mut class = [0u16; 256];
        let class_len = GetClassNameW(hwnd, &mut class);
        let class_name = String::from_utf16_lossy(&class[..class_len.max(0) as usize]);
        let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
        let output = &mut *(lparam.0 as *mut Vec<WindowInfo>);
        output.push(WindowInfo {
            hwnd: hwnd.0 as isize,
            title,
            executable_name,
            executable_path: path,
            class_name,
            is_borderless: style & BORDER_STYLES == 0,
        });
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
    fn detects_sparse_icon_canvas() {
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
        assert!(visible_pixel_ratio(&icon) < 0.2);
    }

    #[test]
    fn limits_dragging_to_the_original_title_bar_height() {
        let rect = RECT {
            left: 100,
            top: 200,
            right: 900,
            bottom: 700,
        };
        assert!(is_drag_point(POINT { x: 400, y: 224 }, rect, 32));
        assert!(!is_drag_point(POINT { x: 400, y: 240 }, rect, 32));
        assert!(!is_drag_point(POINT { x: 99, y: 224 }, rect, 32));
    }

    #[test]
    fn dragging_preserves_the_click_offset_in_both_axes() {
        let drag = ActiveDrag {
            hwnd: 1,
            cursor_origin: POINT { x: 350, y: 220 },
            window_left: 100,
            window_top: 200,
        };
        assert_eq!(drag_position(drag, POINT { x: 400, y: 280 }), (150, 260));
        assert_eq!(drag_position(drag, drag.cursor_origin), (100, 200));
    }

    #[test]
    fn extracts_a_renderable_executable_icon() {
        let path = std::env::current_exe().unwrap();
        let icon = executable_icon(path.to_str().unwrap()).expect("executable icon");
        assert!(icon.width >= 32);
        assert_eq!(icon.rgba.len(), (icon.width * icon.height * 4) as usize);
    }
}
