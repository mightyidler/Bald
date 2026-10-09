//! Bounded, explicitly authorized live-game style isolation. Never in the app build.
use super::*;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, GetDC, ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::UI::{
    Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEINPUT,
        SendInput,
    },
    WindowsAndMessaging::{
        GA_ROOT, GUITHREADINFO, GetAncestor, GetCursorPos, GetGUIThreadInfo, SW_RESTORE,
        SetCursorPos, SetForegroundWindow, ShowWindowAsync, WindowFromPoint,
    },
};

#[derive(Clone, Copy)]
struct Geometry {
    outer: RECT,
    client: RECT,
    origin: POINT,
}

fn geometry(hwnd: HWND) -> Result<Geometry> {
    let mut value = Geometry {
        outer: RECT::default(),
        client: RECT::default(),
        origin: POINT::default(),
    };
    unsafe {
        GetWindowRect(hwnd, &mut value.outer)?;
        GetClientRect(hwnd, &mut value.client)?;
        ClientToScreen(hwnd, &mut value.origin).ok()?;
    }
    Ok(value)
}

fn button(down: bool) -> Result<()> {
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dwFlags: if down {
                    MOUSEEVENTF_LEFTDOWN
                } else {
                    MOUSEEVENTF_LEFTUP
                },
                ..Default::default()
            },
        },
    };
    if unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) } != 1 {
        bail!("native mouse input was rejected");
    }
    Ok(())
}

struct PressedButton;
impl Drop for PressedButton {
    fn drop(&mut self) {
        let _ = button(false);
    }
}

fn drag(hwnd: HWND, y: i32, label: &str) -> Result<(bool, i32)> {
    drag_distance(hwnd, y, label, 40, 144)
}

fn drag_distance(hwnd: HWND, y: i32, label: &str, dx: i32, dy: i32) -> Result<(bool, i32)> {
    if unsafe { GetForegroundWindow() } != hwnd
        || unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } < 0
    {
        bail!("foreground or mouse state changed before isolated drag");
    }
    let before = geometry(hwnd)?;
    let x = before.outer.left + (before.outer.right - before.outer.left) / 2;
    unsafe {
        SetCursorPos(x, y)?;
    }
    std::thread::sleep(Duration::from_millis(80));
    let mut cursor = POINT::default();
    unsafe {
        GetCursorPos(&mut cursor)?;
    }
    if (cursor.x, cursor.y) != (x, y) {
        bail!("game constrained cursor before drag, no input sent");
    }
    button(true)?;
    let held = PressedButton;
    std::thread::sleep(Duration::from_millis(80));
    let thread = unsafe { GetWindowThreadProcessId(hwnd, None) };
    let mut captured = false;
    for step in 0..9 {
        if unsafe { GetForegroundWindow() } != hwnd
            || !unsafe { IsWindowVisible(hwnd).as_bool() }
            || unsafe { IsIconic(hwnd).as_bool() }
        {
            bail!("target became inactive or invisible during drag");
        }
        if step > 0 {
            unsafe {
                SetCursorPos(x + step * dx / 8, y + step * dy / 8)?;
            }
        }
        std::thread::sleep(Duration::from_millis(40));
        let current = geometry(hwnd)?;
        let mut info = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        unsafe {
            GetGUIThreadInfo(thread, &mut info)?;
        }
        captured |= info.hwndCapture == hwnd;
        println!(
            "{label} step={step} outer={},{} client={}x{} origin={},{} capture={:x} gui={:x}",
            current.outer.left,
            current.outer.top,
            current.client.right,
            current.client.bottom,
            current.origin.x,
            current.origin.y,
            info.hwndCapture.0 as usize,
            info.flags.0
        );
    }
    drop(held);
    std::thread::sleep(Duration::from_millis(120));
    let after = geometry(hwnd)?;
    Ok((captured, after.outer.top - before.outer.top))
}

fn reset_position(hwnd: HWND, initial: Geometry) -> Result<()> {
    unsafe {
        SetWindowPos(
            hwnd,
            None,
            initial.outer.left,
            initial.outer.top,
            initial.outer.right - initial.outer.left,
            initial.outer.bottom - initial.outer.top,
            QUIET_POSITION | SWP_ASYNCWINDOWPOS,
        )?;
    }
    wait_for_frame_geometry(
        hwnd,
        initial.outer.left,
        initial.outer.top,
        initial.outer.right - initial.outer.left,
        initial.outer.bottom - initial.outer.top,
    )
}

fn restore_minimized(hwnd: HWND) -> Result<()> {
    unsafe {
        ShowWindowAsync(hwnd, SW_RESTORE).ok()?;
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while unsafe { IsIconic(hwnd).as_bool() } && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    if unsafe { IsIconic(hwnd).as_bool() } || !unsafe { IsWindowVisible(hwnd).as_bool() } {
        bail!("owner-thread normal restore did not show game");
    }
    Ok(())
}

fn capture_top(hwnd: HWND, label: &str) -> Result<()> {
    let frame = geometry(hwnd)?.outer;
    let width = frame.right - frame.left;
    let height = 100;
    unsafe {
        let screen = GetDC(None);
        let memory = CreateCompatibleDC(Some(screen));
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let bitmap = CreateDIBSection(Some(screen), &info, DIB_RGB_COLORS, &mut bits, None, 0)?;
        let prior = SelectObject(memory, bitmap.into());
        let copied = BitBlt(
            memory,
            0,
            0,
            width,
            height,
            Some(screen),
            frame.left,
            frame.top,
            SRCCOPY | CAPTUREBLT,
        );
        let mut pixels =
            std::slice::from_raw_parts(bits as *const u8, (width * height * 4) as usize).to_vec();
        SelectObject(memory, prior);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
        copied?;
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.swap(0, 2);
            pixel[3] = 255;
        }
        image::RgbaImage::from_raw(width as u32, height as u32, pixels)
            .unwrap()
            .save(format!(
                "C:/Projects/Bald/artifacts/bald-test-session/{label}.png"
            ))?;
    }
    Ok(())
}

fn native_caption_candidate(hwnd: HWND, initial: Geometry) -> Result<()> {
    let controller = WindowController::default();
    let trial = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<()> {
        capture_top(hwnd, "caption-before")?;
        controller.make_borderless(hwnd.0 as isize)?;
        let native = geometry(hwnd)?;
        println!(
            "native-disabled client={}x{} inset={} rendering={:?}",
            native.client.right,
            native.client.bottom,
            native.origin.y - native.outer.top,
            native_frame_rendering(hwnd)
        );
        std::thread::sleep(Duration::from_millis(200));
        capture_top(hwnd, "caption-clipped-disabled")?;
        let applied = geometry(hwnd)?;
        let (captured, dy) = drag(hwnd, applied.origin.y + 25, "native-caption-below-strip")?;
        println!("NATIVE below-strip captured={captured} dy={dy}");
        if captured || dy != 0 {
            bail!("native-caption-preserving candidate still allows game drag");
        }
        let window = enumerate_windows()
            .into_iter()
            .find(|window| window.hwnd == hwnd.0 as isize)
            .unwrap();
        let mut rule = crate::rules::ApplicationRule::from_window(&window);
        rule.executable_path = None;
        rule.drag_mode = DragMode::Enabled;
        let config = Arc::new(std::sync::RwLock::new(crate::config::Config {
            applications: vec![rule],
            ..Default::default()
        }));
        let statuses = Arc::new(std::sync::RwLock::new(HashMap::new()));
        let watcher = crate::watcher::Watcher::start(config.clone(), statuses, controller.clone());
        std::thread::sleep(Duration::from_millis(200));
        let current = geometry(hwnd)?;
        let (captured, dy) = drag(hwnd, current.origin.y + 5, "native-caption-bald-strip")?;
        println!("NATIVE bald-strip captured={captured} dy={dy}");
        if captured || dy < 80 {
            bail!("real Bald hook did not move native-caption candidate vertically");
        }
        let current = geometry(hwnd)?;
        let (captured, dy) = drag_distance(
            hwnd,
            current.origin.y + 5,
            "native-caption-to-screen-top",
            0,
            -current.origin.y,
        )?;
        let top = geometry(hwnd)?;
        println!(
            "NATIVE screen-top captured={captured} dy={dy} clientY={}",
            top.origin.y
        );
        if captured || top.origin.y != 0 {
            bail!("native-caption candidate leaves a gap at screen top");
        }
        config.write().unwrap().applications[0].drag_mode = DragMode::Disabled;
        watcher.notify();
        std::thread::sleep(Duration::from_millis(250));
        controller.center_blocked_window(hwnd.0 as isize)?;
        let centered = geometry(hwnd)?;
        println!(
            "NATIVE centered clientOrigin={},{}",
            centered.origin.x, centered.origin.y
        );
        for offset in [5, 25] {
            let current = geometry(hwnd)?;
            let (captured, dy) = drag(hwnd, current.origin.y + offset, "native-caption-blocked")?;
            println!("NATIVE blocked offset={offset} captured={captured} dy={dy}");
            if captured || dy != 0 {
                bail!("blocked candidate moved");
            }
        }
        drop(watcher);
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWMINNOACTIVE);
        }
        std::thread::sleep(Duration::from_millis(150));
        let minimized_visible = unsafe { IsWindowVisible(hwnd).as_bool() };
        let minimized_iconic = unsafe { IsIconic(hwnd).as_bool() };
        println!("NATIVE minimized visible={minimized_visible} iconic={minimized_iconic}");
        restore_minimized(hwnd)?;
        std::thread::sleep(Duration::from_millis(150));
        let shown = geometry(hwnd)?;
        if !minimized_visible
            || !minimized_iconic
            || !unsafe { IsWindowVisible(hwnd).as_bool() }
            || unsafe { IsIconic(hwnd).as_bool() }
            || (shown.client.right, shown.client.bottom)
                != (initial.client.right, initial.client.bottom)
        {
            bail!("native-caption minimize/restore did not preserve visibility and render size");
        }
        Ok(())
    }));
    let restored = controller.restore_all_borders();
    if unsafe { IsIconic(hwnd).as_bool() } {
        restore_minimized(hwnd)?;
    }
    std::thread::sleep(Duration::from_millis(150));
    reset_position(hwnd, initial)?;
    if trial.as_ref().is_ok_and(|result| result.is_ok()) {
        for cycle in 0..3 {
            if !controller.make_borderless(hwnd.0 as isize)? {
                bail!("repeat application was not applied");
            }
            if controller.make_borderless(hwnd.0 as isize)? {
                bail!("duplicate application was not a no-op");
            }
            controller.restore_all_borders()?;
            let restored_content = geometry(hwnd)?;
            println!(
                "NATIVE repeat cycle={cycle} client={}x{} origin={},{} rendering={:?}",
                restored_content.client.right,
                restored_content.client.bottom,
                restored_content.origin.x,
                restored_content.origin.y,
                native_frame_rendering(hwnd)
            );
            if (
                restored_content.client.right,
                restored_content.client.bottom,
                restored_content.origin.x,
                restored_content.origin.y,
            ) != (
                initial.client.right,
                initial.client.bottom,
                initial.origin.x,
                initial.origin.y,
            ) || native_frame_rendering(hwnd) != Some(true)
            {
                bail!("repeat frame restoration changed original content or rendering");
            }
        }
    }
    capture_top(hwnd, "caption-after")?;
    println!(
        "NATIVE cleanup={restored:?} rendering={:?}",
        native_frame_rendering(hwnd)
    );
    restored?;
    match trial {
        Ok(result) => result,
        Err(error) => std::panic::resume_unwind(error),
    }
}

fn apply_partial(
    controller: &WindowController,
    hwnd: HWND,
    initial: Geometry,
    style: i32,
    ex: i32,
    style_mask: u32,
    ex_mask: u32,
) -> Result<()> {
    controller.original_windows.lock().unwrap().insert(
        hwnd.0 as isize,
        OriginalWindowState {
            process_id: window_process_id(hwnd),
            style,
            ex_style: ex,
            native_rendering: native_frame_rendering(hwnd),
            clipped: false,
            native_region: false,
            original_region: None,
            clipped_size: None,
            clipped_content_size: None,
            frame_insets: RECT {
                left: initial.origin.x - initial.outer.left,
                top: initial.origin.y - initial.outer.top,
                right: initial.outer.right - initial.origin.x - initial.client.right,
                bottom: initial.outer.bottom - initial.origin.y - initial.client.bottom,
            },
            restoring: false,
        },
    );
    unsafe {
        SetPropW(
            hwnd,
            w!("Bald.OriginalStyle"),
            Some(HANDLE(style as isize as *mut c_void)),
        )?;
        SetPropW(
            hwnd,
            w!("Bald.OriginalExStyle"),
            Some(HANDLE(ex as isize as *mut c_void)),
        )?;
    }
    set_style_checked(hwnd, (style as u32 & !style_mask) as i32)?;
    set_window_long_checked(hwnd, GWL_EXSTYLE, (ex as u32 & !ex_mask) as i32)?;
    refresh_native_frame(hwnd)?;
    std::thread::sleep(Duration::from_millis(150));
    let updated = geometry(hwnd)?;
    let x = initial.origin.x - (updated.origin.x - updated.outer.left);
    let y = initial.origin.y - (updated.origin.y - updated.outer.top);
    let width =
        initial.client.right + updated.outer.right - updated.outer.left - updated.client.right;
    let height =
        initial.client.bottom + updated.outer.bottom - updated.outer.top - updated.client.bottom;
    unsafe {
        SetWindowPos(
            hwnd,
            None,
            x,
            y,
            width,
            height,
            QUIET_POSITION | SWP_ASYNCWINDOWPOS,
        )?;
    }
    wait_for_frame_geometry(hwnd, x, y, width, height)?;
    let final_state = geometry(hwnd)?;
    if (
        final_state.client.right,
        final_state.client.bottom,
        final_state.origin.x,
        final_state.origin.y,
    ) != (
        initial.client.right,
        initial.client.bottom,
        initial.origin.x,
        initial.origin.y,
    ) {
        bail!("partial style change did not preserve content geometry");
    }
    Ok(())
}

pub(super) fn run(hwnd: HWND, authorization: &std::path::Path) -> Result<()> {
    let permit: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(authorization)?)?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let pid = window_process_id(hwnd);
    let expiry = permit["expires"].as_u64().unwrap_or(0);
    if permit["hwnd"].as_i64() != Some(hwnd.0 as i64)
        || permit["pid"].as_u64() != Some(pid as u64)
        || expiry <= now
        || expiry > now + 3600
    {
        bail!("no fresh authorization for this exact game window");
    }
    if !unsafe { IsWindowVisible(hwnd).as_bool() }
        || unsafe { IsIconic(hwnd).as_bool() }
        || !unsafe { GetPropW(hwnd, w!("Bald.OriginalStyle")).0.is_null() }
    {
        bail!("clean visible game required before drag trial");
    }
    let initial = geometry(hwnd)?;
    let style = unsafe { GetWindowLongW(hwnd, GWL_STYLE) };
    let ex = unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) };
    let foreground = unsafe { GetForegroundWindow() };
    let mut cursor = POINT::default();
    unsafe {
        GetCursorPos(&mut cursor)?;
    }
    let controller = WindowController::default();
    let trial = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<()> {
        if unsafe { GetForegroundWindow() } != hwnd
            && !unsafe { SetForegroundWindow(hwnd).as_bool() }
        {
            let mut activated = false;
            for offset in [60, 240, 480, 720, 960] {
                let point = POINT {
                    x: initial.outer.left + offset,
                    y: initial.outer.top + (initial.origin.y - initial.outer.top) / 2,
                };
                if unsafe { GetAncestor(WindowFromPoint(point), GA_ROOT) } != hwnd {
                    continue;
                }
                if unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } < 0 {
                    bail!("mouse already pressed before activation");
                }
                unsafe {
                    SetCursorPos(point.x, point.y)?;
                }
                button(true)?;
                let held = PressedButton;
                std::thread::sleep(Duration::from_millis(80));
                drop(held);
                std::thread::sleep(Duration::from_millis(150));
                activated = unsafe { GetForegroundWindow() } == hwnd;
                break;
            }
            if !activated {
                bail!("game caption is covered; no clicks sent to other applications");
            }
        }
        std::thread::sleep(Duration::from_millis(250));
        let (captured, dy) = drag(
            hwnd,
            initial.outer.top + (initial.origin.y - initial.outer.top) / 2,
            "original_caption",
        )?;
        println!("control captured={captured} dy={dy}");
        reset_position(hwnd, initial)?;
        if !captured || dy < 80 {
            bail!("synthetic input did not reproduce original native drag; matrix aborted");
        }
        if permit["trial"].as_str() == Some("native_caption") {
            return native_caption_candidate(hwnd, initial);
        }
        for (label, style_mask, ex_mask) in [
            ("thickframe_only", WS_THICKFRAME.0, 0),
            ("caption_only", WS_CAPTION.0, 0),
            ("windowedge_only", 0, WS_EX_WINDOWEDGE.0),
            ("styles_keep_edge", BORDER_STYLES, 0),
            ("full_async_candidate", BORDER_STYLES, BORDER_EX_STYLES),
        ] {
            if style_mask == BORDER_STYLES && ex_mask == BORDER_EX_STYLES {
                controller.make_borderless_style(hwnd.0 as isize)?;
            } else {
                apply_partial(&controller, hwnd, initial, style, ex, style_mask, ex_mask)?;
            }
            let applied = geometry(hwnd)?;
            println!(
                "CASE {label} style={:08x} ex={:08x} client={}x{} inset={}",
                unsafe { GetWindowLongW(hwnd, GWL_STYLE) },
                unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) },
                applied.client.right,
                applied.client.bottom,
                applied.origin.y - applied.outer.top
            );
            let (captured, dy) = drag(hwnd, applied.origin.y + 25, label)?;
            println!("RESULT {label} captured={captured} dy={dy}");
            controller.restore_all_borders()?;
            reset_position(hwnd, initial)?;
            if unsafe { GetWindowLongW(hwnd, GWL_STYLE) } != style
                || unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) } != ex
                || native_frame_rendering(hwnd) != Some(true)
            {
                bail!("original frame not restored, remaining cases aborted");
            }
        }
        native_caption_candidate(hwnd, initial)?;
        Ok(())
    }));
    let restored = controller.restore_all_borders();
    if window_process_id(hwnd) == pid && restored.is_ok() {
        reset_position(hwnd, initial)?;
    }
    unsafe {
        if GetForegroundWindow() == hwnd {
            let _ = SetForegroundWindow(foreground);
            let _ = SetCursorPos(cursor.x, cursor.y);
        }
    }
    println!(
        "matrix cleanup={restored:?} style={:08x} native={:?}",
        unsafe { GetWindowLongW(hwnd, GWL_STYLE) },
        native_frame_rendering(hwnd)
    );
    restored?;
    match trial {
        Ok(result) => result,
        Err(error) => std::panic::resume_unwind(error),
    }
}
