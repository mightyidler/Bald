//! Real Win32 regression fixtures, entirely separate from the user's games.
//! No focus, keyboard/mouse injection, SC_MOVE or SC_RESTORE is used here.
use super::*;
use std::sync::Once;
use windows::Win32::{
    Foundation::HINSTANCE,
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext},
        WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, NCCALCSIZE_PARAMS, RegisterClassW,
            STYLESTRUCT, SW_HIDE, SW_SHOWMINNOACTIVE, SW_SHOWNOACTIVATE, SetWindowTextW,
            ShowWindow, WINDOWPOS, WM_NCCALCSIZE, WM_STYLECHANGED, WM_WINDOWPOSCHANGING, WNDCLASSW,
            WS_EX_NOACTIVATE, WS_OVERLAPPEDWINDOW, WS_POPUP, WS_VISIBLE,
        },
    },
};

#[test]
#[ignore = "read-only live permission inspection; requires BALD_PERMISSION_TEST_HWND"]
fn elevated_target_is_detected_before_any_window_mutation() {
    let value: isize = std::env::var("BALD_PERMISSION_TEST_HWND")
        .expect("target HWND")
        .parse()
        .unwrap();
    let hwnd = HWND(value as *mut c_void);
    assert!(unsafe { IsWindow(Some(hwnd)).as_bool() });
    assert!(requires_elevation(hwnd));
    let before_style = unsafe { GetWindowLongW(hwnd, GWL_STYLE) };
    let before_marker = unsafe { GetPropW(hwnd, w!("Bald.ClippedFrame")) };
    let error = WindowController::default()
        .make_borderless(value)
        .unwrap_err();
    assert!(error.to_string().contains("elevation_required"));
    assert_eq!(unsafe { GetWindowLongW(hwnd, GWL_STYLE) }, before_style);
    assert_eq!(
        unsafe { GetPropW(hwnd, w!("Bald.ClippedFrame")) },
        before_marker
    );
    let found = enumerate_windows()
        .into_iter()
        .find(|window| window.hwnd == value)
        .expect("target detected by watcher enumeration");
    assert_eq!(found.class_name, "MapleStoryClass");
    assert_eq!(found.executable_name, "MapleStory.exe");
    println!("target detected; elevation_required; native window unchanged");
}

struct Fixture(HWND);

#[path = "maple_style_trials.rs"]
mod maple_style_trials;

#[test]
#[ignore = "approved live owner-thread frame trial; requires BALD_NATIVE_GAME_TEST_HWND"]
fn live_maple_owner_frame_trial() {
    unsafe {
        let dpi = windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        );
        println!("live test process DPI initialization={dpi:?}");
        SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    assert!(
        enumerate_windows()
            .iter()
            .all(|window| window.executable_name != "bald.exe")
    );
    let value: isize = std::env::var("BALD_NATIVE_GAME_TEST_HWND")
        .unwrap()
        .parse()
        .unwrap();
    let hwnd = HWND(value as *mut c_void);
    assert!(is_native_region_game(hwnd));
    let authorization = std::path::Path::new(
        "C:/Projects/Bald/artifacts/bald-test-session/native-drag-authorization.json",
    );
    if authorization.exists() {
        maple_style_trials::run(hwnd, authorization).unwrap();
        return;
    }
    assert!(unsafe { IsWindowVisible(hwnd).as_bool() && !IsIconic(hwnd).as_bool() });
    assert!(unsafe { GetPropW(hwnd, w!("Bald.OriginalStyle")).0.is_null() });
    assert!(unsafe { GetPropW(hwnd, w!("Bald.NativeFrameRegion")).0.is_null() });
    let style = unsafe { GetWindowLongW(hwnd, GWL_STYLE) };
    let ex = unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) };
    let mut outer = RECT::default();
    let mut content = RECT::default();
    let mut origin = POINT::default();
    unsafe {
        GetWindowRect(hwnd, &mut outer).unwrap();
        GetClientRect(hwnd, &mut content).unwrap();
        ClientToScreen(hwnd, &mut origin).ok().unwrap();
    }
    assert!(style as u32 & WS_CAPTION.0 == WS_CAPTION.0 && origin.y > outer.top);
    assert_eq!(native_frame_rendering(hwnd), Some(true));
    let controller = WindowController::default();
    let trial = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        for cycle in 0..3 {
            controller.make_borderless(value).unwrap();
            let mut client = RECT::default();
            let mut applied_origin = POINT::default();
            unsafe {
                GetClientRect(hwnd, &mut client).unwrap();
                ClientToScreen(hwnd, &mut applied_origin).ok().unwrap();
            }
            println!(
                "cycle={cycle} apply client={}x{} origin={},{}",
                client.right, client.bottom, applied_origin.x, applied_origin.y
            );
            assert_eq!(
                (client.right, client.bottom),
                (content.right, content.bottom)
            );
            assert_eq!((applied_origin.x, applied_origin.y), (origin.x, origin.y));
            controller.restore_borders(value).unwrap();
            applied_origin = POINT::default();
            unsafe {
                GetClientRect(hwnd, &mut client).unwrap();
                ClientToScreen(hwnd, &mut applied_origin).ok().unwrap();
            }
            println!(
                "cycle={cycle} restore client={}x{} origin={},{} native={:?}",
                client.right,
                client.bottom,
                applied_origin.x,
                applied_origin.y,
                native_frame_rendering(hwnd)
            );
            assert_eq!(
                (client.right, client.bottom),
                (content.right, content.bottom)
            );
            assert_eq!((applied_origin.x, applied_origin.y), (origin.x, origin.y));
            assert_eq!(unsafe { GetWindowLongW(hwnd, GWL_STYLE) }, style);
            assert_eq!(unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) }, ex);
            assert_eq!(native_frame_rendering(hwnd), Some(true));
        }
    }));
    // A failed candidate never intentionally leaves borderless state behind.
    let cleanup = controller.restore_all_borders();
    println!("cleanup={cleanup:?}");
    cleanup.unwrap();
    if let Err(error) = trial {
        std::panic::resume_unwind(error);
    }
    println!("production native-frame cycles verified; real input requires fresh authorization");
}

#[test]
#[ignore = "approved live native-frame roundtrip; requires BALD_NATIVE_GAME_TEST_HWND"]
fn live_maple_native_frame_roundtrip() {
    unsafe {
        SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let value: isize = std::env::var("BALD_NATIVE_GAME_TEST_HWND")
        .unwrap()
        .parse()
        .unwrap();
    let hwnd = HWND(value as *mut c_void);
    assert!(is_native_region_game(hwnd));
    let style = unsafe { GetWindowLongW(hwnd, GWL_STYLE) };
    let ex = unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) };
    let mut before = RECT::default();
    unsafe {
        GetWindowRect(hwnd, &mut before).unwrap();
    }
    let controller = WindowController::default();
    assert!(controller.make_borderless(value).unwrap());
    assert_eq!(unsafe { GetWindowLongW(hwnd, GWL_STYLE) }, style);
    assert_eq!(unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) }, ex);
    assert_eq!(native_frame_rendering(hwnd), Some(false));
    controller.restore_all_borders().unwrap();
    assert_eq!(native_frame_rendering(hwnd), Some(true));
    let mut after = RECT::default();
    unsafe {
        GetWindowRect(hwnd, &mut after).unwrap();
    }
    assert_eq!(
        (before.left, before.top, before.right, before.bottom),
        (after.left, after.top, after.right, after.bottom)
    );
    assert!(controller.managed_windows().is_empty());
    println!("native caption style retained; frame rendering and region restored");
}

unsafe extern "system" fn fixture_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    if message == windows::Win32::UI::WindowsAndMessaging::WM_GETTEXT {
        unsafe {
            let count = GetPropW(hwnd, w!("Bald.TestTitleRequests")).0 as usize;
            SetPropW(
                hwnd,
                w!("Bald.TestTitleRequests"),
                Some(HANDLE((count + 1) as *mut c_void)),
            )
            .unwrap();
        }
    }
    if message == WM_STYLECHANGED
        && unsafe { !GetPropW(hwnd, w!("Bald.TestDelayedCaption")).0.is_null() }
        && unsafe { GetWindowLongW(hwnd, GWL_STYLE) as u32 & WS_CAPTION.0 == WS_CAPTION.0 }
    {
        unsafe {
            let count = GetPropW(hwnd, w!("Bald.TestDelayedStyleCount")).0 as usize;
            SetPropW(
                hwnd,
                w!("Bald.TestDelayedStyleCount"),
                Some(HANDLE((count + 1) as *mut c_void)),
            )
            .unwrap();
            SetPropW(
                hwnd,
                w!("Bald.TestCaptionPending"),
                Some(HANDLE(std::ptr::dangling_mut())),
            )
            .unwrap();
            windows::Win32::UI::WindowsAndMessaging::SetTimer(Some(hwnd), 73, 200, None);
        }
    }
    if message == windows::Win32::UI::WindowsAndMessaging::WM_TIMER && wparam.0 == 73 {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::KillTimer(Some(hwnd), 73);
            if !GetPropW(hwnd, w!("Bald.TestMinimizeDuringRestore"))
                .0
                .is_null()
            {
                let _ = ShowWindow(hwnd, SW_SHOWMINNOACTIVE);
                return windows::Win32::Foundation::LRESULT(0);
            }
            let _ = RemovePropW(hwnd, w!("Bald.TestCaptionPending"));
        }
        refresh_native_frame(hwnd).unwrap();
        return windows::Win32::Foundation::LRESULT(0);
    }
    if message == WM_NCCALCSIZE
        && unsafe { !GetPropW(hwnd, w!("Bald.TestCaptionPending")).0.is_null() }
    {
        return windows::Win32::Foundation::LRESULT(0);
    }
    if message == WM_STYLECHANGED
        && unsafe {
            !GetPropW(hwnd, w!("Bald.TestNeedsFrameNotification"))
                .0
                .is_null()
        }
    {
        unsafe {
            let _ = SetPropW(
                hwnd,
                w!("Bald.TestPendingFrame"),
                Some(HANDLE(std::ptr::dangling_mut())),
            );
        }
    }
    if message == WM_WINDOWPOSCHANGING
        && unsafe { (*(lparam.0 as *const WINDOWPOS)).flags.0 & SWP_FRAMECHANGED.0 != 0 }
    {
        unsafe {
            let _ = RemovePropW(hwnd, w!("Bald.TestPendingFrame"));
        }
    }
    if message == WM_NCCALCSIZE
        && unsafe { !GetPropW(hwnd, w!("Bald.TestPendingFrame")).0.is_null() }
    {
        return windows::Win32::Foundation::LRESULT(0);
    }
    if message == WM_WINDOWPOSCHANGING
        && unsafe { !GetPropW(hwnd, w!("Bald.TestClampPosition")).0.is_null() }
    {
        let position = unsafe { &mut *(lparam.0 as *mut WINDOWPOS) };
        if position.flags.0 & SWP_NOMOVE.0 == 0 {
            position.y = 0;
        }
    }
    if message == WM_NCCALCSIZE
        && unsafe {
            !GetPropW(hwnd, w!("Bald.TestRejectNativeCaption"))
                .0
                .is_null()
        }
    {
        return windows::Win32::Foundation::LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}
unsafe extern "system" fn region_sensitive_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    let sticky = unsafe { !GetPropW(hwnd, w!("Bald.TestStickyFrame")).0.is_null() };
    if message == WM_WINDOWPOSCHANGING
        && unsafe { !GetPropW(hwnd, w!("Bald.TestCountRestoreSizes")).0.is_null() }
        && unsafe { (*(lparam.0 as *const WINDOWPOS)).flags.0 & SWP_NOSIZE.0 == 0 }
    {
        unsafe {
            let count = GetPropW(hwnd, w!("Bald.TestRestoreSizeCount")).0 as usize;
            let _ = SetPropW(
                hwnd,
                w!("Bald.TestRestoreSizeCount"),
                Some(HANDLE((count + 1) as *mut c_void)),
            );
        }
    }
    if sticky
        && message == WM_STYLECHANGED
        && unsafe {
            (*(lparam.0 as *const STYLESTRUCT)).styleOld
                != (*(lparam.0 as *const STYLESTRUCT)).styleNew
        }
        && unsafe {
            GetPropW(hwnd, w!("Bald.TestRefuseFrameRestore"))
                .0
                .is_null()
        }
        && unsafe {
            GetPropW(hwnd, w!("Bald.TestRequireOuterResize"))
                .0
                .is_null()
        }
    {
        unsafe {
            let _ = RemovePropW(hwnd, w!("Bald.TestCachedFrame"));
        }
    }
    if message == WM_NCCALCSIZE {
        let region = unsafe { CreateRectRgn(0, 0, 0, 0) };
        let clipped = unsafe { GetWindowRgn(hwnd, region).0 != 0 };
        unsafe {
            let _ = DeleteObject(region.into());
        }
        if clipped && sticky {
            unsafe {
                let _ = SetPropW(
                    hwnd,
                    w!("Bald.TestCachedFrame"),
                    Some(HANDLE(1usize as *mut c_void)),
                );
            }
        }
        if sticky
            && !clipped
            && unsafe {
                !GetPropW(hwnd, w!("Bald.TestRequireOuterResize"))
                    .0
                    .is_null()
            }
            && unsafe { GetWindowLongW(hwnd, GWL_STYLE) as u32 & WS_CAPTION.0 == WS_CAPTION.0 }
        {
            let requested = if wparam.0 != 0 {
                unsafe { (*(lparam.0 as *const NCCALCSIZE_PARAMS)).rgrc[0] }
            } else {
                unsafe { *(lparam.0 as *const RECT) }
            };
            let expected_width =
                unsafe { GetPropW(hwnd, w!("Bald.TestOuterWidth")).0 as isize as i32 };
            let expected_height =
                unsafe { GetPropW(hwnd, w!("Bald.TestOuterHeight")).0 as isize as i32 };
            if (
                requested.right - requested.left,
                requested.bottom - requested.top,
            ) == (expected_width, expected_height)
            {
                unsafe {
                    let _ = RemovePropW(hwnd, w!("Bald.TestCachedFrame"));
                }
            }
        }
        if clipped || (sticky && unsafe { !GetPropW(hwnd, w!("Bald.TestCachedFrame")).0.is_null() })
        {
            return windows::Win32::Foundation::LRESULT(0);
        }
    }
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

#[test]
fn restoration_refreshes_cached_nonclient_frame_before_forgetting_window() {
    let window = Fixture::new_kind(WS_OVERLAPPEDWINDOW, true);
    unsafe {
        SetPropW(
            window.0,
            w!("Bald.TestStickyFrame"),
            Some(HANDLE(1usize as *mut c_void)),
        )
        .unwrap();
    }
    let controller = WindowController::default();
    let (width, height, _) = window.client();
    controller.clip_game_frame(window.value()).unwrap();
    controller.restore_all_borders().unwrap();
    let (restored_width, restored_height, origin) = window.client();
    let mut outer = RECT::default();
    unsafe {
        GetWindowRect(window.0, &mut outer).unwrap();
    }
    assert!(
        origin.y > outer.top,
        "native caption must exist, not merely its style bit"
    );
    assert_eq!((restored_width, restored_height), (width, height));
    assert!(controller.managed_windows().is_empty());
}

#[test]
fn incomplete_caption_restore_retains_state_and_can_be_retried() {
    let window = Fixture::new_kind(WS_OVERLAPPEDWINDOW, true);
    unsafe {
        SetPropW(
            window.0,
            w!("Bald.TestStickyFrame"),
            Some(HANDLE(1usize as *mut c_void)),
        )
        .unwrap();
        SetPropW(
            window.0,
            w!("Bald.TestRefuseFrameRestore"),
            Some(HANDLE(1usize as *mut c_void)),
        )
        .unwrap();
    }
    let controller = WindowController::default();
    controller.clip_game_frame(window.value()).unwrap();
    assert!(
        controller
            .restore_all_borders()
            .unwrap_err()
            .to_string()
            .contains("caption restoration incomplete")
    );
    assert_eq!(controller.managed_windows().len(), 1);
    assert!(!controller.has_drag_targets());
    unsafe {
        let _ = RemovePropW(window.0, w!("Bald.TestRefuseFrameRestore"));
    }
    controller.restore_all_borders().unwrap();
    assert!(controller.managed_windows().is_empty());
}

#[test]
fn cached_frame_requiring_real_outer_resize_restores_current_content() {
    let window = Fixture::new_kind(WS_OVERLAPPEDWINDOW, true);
    let (width, height, content_origin) = window.client();
    let mut outer = RECT::default();
    unsafe {
        GetWindowRect(window.0, &mut outer).unwrap();
        for (name, value) in [
            (w!("Bald.TestStickyFrame"), 1),
            (w!("Bald.TestRequireOuterResize"), 1),
            (w!("Bald.TestOuterWidth"), outer.right - outer.left),
            (w!("Bald.TestOuterHeight"), outer.bottom - outer.top),
        ] {
            SetPropW(window.0, name, Some(HANDLE(value as isize as *mut c_void))).unwrap();
        }
    }
    let controller = WindowController::default();
    controller.clip_game_frame(window.value()).unwrap();
    controller.restore_all_borders().unwrap();
    let (actual_width, actual_height, actual_origin) = window.client();
    assert_eq!((actual_width, actual_height), (width, height));
    assert_eq!(
        (actual_origin.x, actual_origin.y),
        (content_origin.x, content_origin.y)
    );
    assert!(controller.managed_windows().is_empty());
}

#[test]
fn failed_frame_restore_does_not_compensate_using_missing_caption() {
    let window = Fixture::new_kind(WS_OVERLAPPEDWINDOW, true);
    unsafe {
        for name in [
            w!("Bald.TestStickyFrame"),
            w!("Bald.TestRefuseFrameRestore"),
        ] {
            SetPropW(window.0, name, Some(HANDLE(1usize as *mut c_void))).unwrap();
        }
    }
    let controller = WindowController::default();
    controller.clip_game_frame(window.value()).unwrap();
    unsafe {
        SetPropW(
            window.0,
            w!("Bald.TestCountRestoreSizes"),
            Some(HANDLE(1usize as *mut c_void)),
        )
        .unwrap();
    }
    assert!(controller.restore_all_borders().is_err());
    // One native outer-size request, then one failure rollback. No third
    // resize based on an un-restored full-window client rectangle.
    assert_eq!(
        unsafe { GetPropW(window.0, w!("Bald.TestRestoreSizeCount")).0 as usize },
        2
    );
    assert_eq!(controller.managed_windows().len(), 1);
}
impl Fixture {
    fn new(style: windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE) -> Self {
        Self::new_kind(style, false)
    }
    fn new_kind(
        style: windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE,
        region_sensitive: bool,
    ) -> Self {
        Self::new_fixture(style, region_sensitive, false)
    }
    fn new_fixture(
        style: windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE,
        region_sensitive: bool,
        maple: bool,
    ) -> Self {
        static REGISTER: Once = Once::new();
        unsafe {
            let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            REGISTER.call_once(|| {
                let module = GetModuleHandleW(None).unwrap();
                assert_ne!(
                    RegisterClassW(&WNDCLASSW {
                        lpfnWndProc: Some(fixture_proc),
                        hInstance: HINSTANCE(module.0),
                        lpszClassName: w!("Bald.RegressionFixture"),
                        ..Default::default()
                    }),
                    0
                );
                assert_ne!(
                    RegisterClassW(&WNDCLASSW {
                        lpfnWndProc: Some(region_sensitive_proc),
                        hInstance: HINSTANCE(module.0),
                        lpszClassName: w!("Bald.RegionSensitiveFixture"),
                        ..Default::default()
                    }),
                    0
                );
                assert_ne!(
                    RegisterClassW(&WNDCLASSW {
                        lpfnWndProc: Some(fixture_proc),
                        hInstance: HINSTANCE(module.0),
                        lpszClassName: w!("MapleStoryClass"),
                        ..Default::default()
                    }),
                    0
                );
            });
            Self(
                CreateWindowExW(
                    WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                    if maple {
                        w!("MapleStoryClass")
                    } else if region_sensitive {
                        w!("Bald.RegionSensitiveFixture")
                    } else {
                        w!("Bald.RegressionFixture")
                    },
                    w!("Bald regression fixture"),
                    style | WS_VISIBLE,
                    -24000,
                    -24000,
                    800,
                    600,
                    None,
                    None,
                    Some(HINSTANCE(GetModuleHandleW(None).unwrap().0)),
                    None,
                )
                .unwrap(),
            )
        }
    }
    fn value(&self) -> isize {
        self.0.0 as isize
    }
    fn client(&self) -> (i32, i32, POINT) {
        unsafe {
            let mut rect = RECT::default();
            let mut origin = POINT::default();
            GetClientRect(self.0, &mut rect).unwrap();
            assert!(ClientToScreen(self.0, &mut origin).as_bool());
            (rect.right, rect.bottom, origin)
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.0);
        }
    }
}

#[test]
fn restoration_does_not_change_unmanaged_popup() {
    let window = Fixture::new(WS_POPUP);
    let before = unsafe { GetWindowLongW(window.0, GWL_STYLE) };
    assert!(
        !WindowController::default()
            .restore_borders(window.value())
            .unwrap()
    );
    assert_eq!(unsafe { GetWindowLongW(window.0, GWL_STYLE) }, before);
}

#[test]
fn restoration_preserves_resized_content_and_client_position() {
    let window = Fixture::new(WS_OVERLAPPEDWINDOW);
    let controller = WindowController::default();
    assert!(controller.make_borderless(window.value()).unwrap());
    unsafe {
        SetWindowPos(
            window.0,
            None,
            -23000,
            -22000,
            900,
            700,
            SWP_NOACTIVATE | SWP_NOZORDER,
        )
        .unwrap();
    }
    let (width, height, origin) = window.client();
    assert_eq!((width, height), (900, 700));
    assert!(controller.restore_borders(window.value()).unwrap());
    let (width, height, after) = window.client();
    assert_eq!((width, height), (900, 700));
    assert_eq!((after.x, after.y), (origin.x, origin.y));
    assert!(unsafe { IsWindowVisible(window.0).as_bool() });
    assert!(!unsafe { IsIconic(window.0).as_bool() });
}

#[test]
fn style_apply_and_restore_bypass_target_position_changing_callback() {
    let window = Fixture::new(WS_OVERLAPPEDWINDOW);
    let before = window.client();
    unsafe {
        SetPropW(
            window.0,
            w!("Bald.TestClampPosition"),
            Some(HANDLE(1usize as *mut c_void)),
        )
        .unwrap();
    }
    let controller = WindowController::default();
    controller.make_borderless(window.value()).unwrap();
    let applied = window.client();
    assert_eq!((applied.2.x, applied.2.y), (before.2.x, before.2.y));
    controller.restore_all_borders().unwrap();
    let restored = window.client();
    assert_eq!((restored.2.x, restored.2.y), (before.2.x, before.2.y));
}

#[test]
fn extended_frame_styles_are_removed_and_restored_without_other_flag_changes() {
    use windows::Win32::UI::WindowsAndMessaging::WS_EX_ACCEPTFILES;
    let window = Fixture::new(WS_OVERLAPPEDWINDOW);
    let original = unsafe { GetWindowLongW(window.0, GWL_EXSTYLE) } as u32
        | BORDER_EX_STYLES
        | WS_EX_ACCEPTFILES.0;
    unsafe {
        SetWindowLongW(window.0, GWL_EXSTYLE, original as i32);
        SetWindowPos(
            window.0,
            None,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | QUIET_POSITION,
        )
        .unwrap();
    }
    let before = window.client();
    let controller = WindowController::default();
    controller.make_borderless(window.value()).unwrap();
    let applied_ex = unsafe { GetWindowLongW(window.0, GWL_EXSTYLE) } as u32;
    assert_eq!(applied_ex, original & !BORDER_EX_STYLES);
    controller.restore_all_borders().unwrap();
    assert_eq!(
        unsafe { GetWindowLongW(window.0, GWL_EXSTYLE) } as u32,
        original
    );
    let restored = window.client();
    assert_eq!(
        (restored.0, restored.1, restored.2.x, restored.2.y),
        (before.0, before.1, before.2.x, before.2.y)
    );
}

#[test]
fn repeated_style_restore_notifies_frame_owner_and_preserves_content() {
    let window = Fixture::new(WS_OVERLAPPEDWINDOW);
    let before = window.client();
    unsafe {
        SetPropW(
            window.0,
            w!("Bald.TestNeedsFrameNotification"),
            Some(HANDLE(std::ptr::dangling_mut())),
        )
        .unwrap();
    }
    let controller = WindowController::default();
    for _ in 0..10 {
        controller.make_borderless(window.value()).unwrap();
        controller.restore_all_borders().unwrap();
        let after = window.client();
        assert_eq!(
            (after.0, after.1, after.2.x, after.2.y),
            (before.0, before.1, before.2.x, before.2.y)
        );
        assert!(controller.managed_windows().is_empty());
    }
}

#[test]
fn modern_frame_rendering_returns_on_each_restore() {
    let window = Fixture::new(WS_OVERLAPPEDWINDOW);
    assert_eq!(native_frame_rendering(window.0), Some(true));
    let controller = WindowController::default();
    for _ in 0..5 {
        controller.make_borderless(window.value()).unwrap();
        let disabled = windows::Win32::Graphics::Dwm::DWMNCRP_DISABLED;
        unsafe {
            DwmSetWindowAttribute(
                window.0,
                DWMWA_NCRENDERING_POLICY,
                &disabled as *const _ as *const c_void,
                std::mem::size_of_val(&disabled) as u32,
            )
            .unwrap();
        }
        assert_eq!(native_frame_rendering(window.0), Some(false));
        controller.restore_all_borders().unwrap();
        assert_eq!(native_frame_rendering(window.0), Some(true));
    }
}

#[test]
fn delayed_owner_frame_is_confirmed_without_reapplying_styles() {
    exercise_deferred_frame(false);
}

#[test]
fn minimization_during_confirmation_does_not_prove_caption_restored() {
    exercise_deferred_frame(true);
}

fn exercise_deferred_frame(minimize_during_restore: bool) {
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, MSG, PostThreadMessageW, WM_QUIT,
    };
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let owner = std::thread::spawn(move || {
        let window = Fixture::new(WS_OVERLAPPEDWINDOW);
        ready_tx
            .send((window.value(), unsafe {
                windows::Win32::System::Threading::GetCurrentThreadId()
            }))
            .unwrap();
        let mut message = MSG::default();
        while unsafe { GetMessageW(&mut message, None, 0, 0).0 } > 0 {
            unsafe {
                DispatchMessageW(&message);
            }
        }
    });
    let (value, thread_id) = ready_rx.recv().unwrap();
    let hwnd = HWND(value as *mut c_void);
    let controller = WindowController::default();
    let result = std::panic::catch_unwind(|| {
        unsafe {
            SetPropW(
                hwnd,
                w!("Bald.TestDelayedCaption"),
                Some(HANDLE(std::ptr::dangling_mut())),
            )
            .unwrap();
            if minimize_during_restore {
                SetPropW(
                    hwnd,
                    w!("Bald.TestMinimizeDuringRestore"),
                    Some(HANDLE(std::ptr::dangling_mut())),
                )
                .unwrap();
            }
        }
        for cycle in 0..3 {
            controller.make_borderless(value).unwrap();
            let mut before = RECT::default();
            unsafe {
                GetClientRect(hwnd, &mut before).unwrap();
            }
            let started = std::time::Instant::now();
            if minimize_during_restore {
                let saved = *controller
                    .original_windows
                    .lock()
                    .unwrap()
                    .get(&value)
                    .unwrap();
                set_window_long_checked(hwnd, GWL_STYLE, saved.style).unwrap();
                refresh_native_frame(hwnd).unwrap();
                let confirmed = wait_for_native_frame(hwnd, saved, false);
                assert!(
                    confirmed
                        .unwrap_err()
                        .to_string()
                        .contains("caption confirmation deferred while minimized")
                );
                assert_eq!(controller.managed_windows().len(), 1);
                assert!(unsafe { IsIconic(hwnd).as_bool() });
                assert!(!unsafe { GetPropW(hwnd, w!("Bald.OriginalStyle")).0.is_null() });
                break;
            }
            controller.restore_borders(value).unwrap();
            assert!(started.elapsed() >= std::time::Duration::from_millis(150));
            assert!(controller.managed_windows().is_empty());
            assert!(unsafe { GetPropW(hwnd, w!("Bald.TestCaptionPending")).0.is_null() });
            assert_eq!(
                unsafe { GetPropW(hwnd, w!("Bald.TestDelayedStyleCount")).0 as usize },
                cycle + 1
            );
            let mut after = RECT::default();
            unsafe {
                GetClientRect(hwnd, &mut after).unwrap();
            }
            assert_eq!((after.right, after.bottom), (before.right, before.bottom));
        }
    });
    unsafe {
        PostThreadMessageW(thread_id, WM_QUIT, WPARAM(0), LPARAM(0)).unwrap();
    }
    owner.join().unwrap();
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}

#[test]
fn failed_caption_from_previous_controller_is_not_used_as_new_baseline() {
    let window = Fixture::new(WS_OVERLAPPEDWINDOW);
    let old = WindowController::default();
    old.make_borderless(window.value()).unwrap();
    unsafe {
        SetPropW(
            window.0,
            w!("Bald.TestRejectNativeCaption"),
            Some(HANDLE(std::ptr::dangling_mut())),
        )
        .unwrap();
    }
    assert!(old.restore_all_borders().is_err());
    let before = window.client();
    let new = WindowController::default();
    assert!(
        new.make_borderless(window.value())
            .unwrap_err()
            .to_string()
            .contains("previous native caption")
    );
    assert!(new.managed_windows().is_empty());
    let after = window.client();
    assert_eq!(
        (before.0, before.1, before.2.x, before.2.y),
        (after.0, after.1, after.2.x, after.2.y)
    );
    unsafe {
        let _ = RemovePropW(window.0, w!("Bald.TestRejectNativeCaption"));
    }
    old.restore_all_borders().unwrap();
    new.make_borderless(window.value()).unwrap();
    new.restore_all_borders().unwrap();
}

#[test]
fn originally_classic_frame_is_not_forced_to_modern_rendering() {
    let window = Fixture::new(WS_OVERLAPPEDWINDOW);
    let disabled = windows::Win32::Graphics::Dwm::DWMNCRP_DISABLED;
    unsafe {
        DwmSetWindowAttribute(
            window.0,
            DWMWA_NCRENDERING_POLICY,
            &disabled as *const _ as *const c_void,
            std::mem::size_of_val(&disabled) as u32,
        )
        .unwrap();
    }
    let controller = WindowController::default();
    controller.make_borderless(window.value()).unwrap();
    controller.restore_all_borders().unwrap();
    assert_eq!(native_frame_rendering(window.0), Some(false));
}

#[test]
fn recovered_legacy_region_markers_do_not_route_new_style_restore_to_clipping() {
    let window = Fixture::new(WS_OVERLAPPEDWINDOW);
    unsafe {
        SetPropW(
            window.0,
            w!("Bald.ClippedFrame"),
            Some(HANDLE(1usize as *mut c_void)),
        )
        .unwrap();
        SetPropW(
            window.0,
            w!("Bald.OriginalStyle"),
            Some(HANDLE(
                GetWindowLongW(window.0, GWL_STYLE) as isize as *mut c_void
            )),
        )
        .unwrap();
    }
    let controller = WindowController::default();
    controller.make_borderless(window.value()).unwrap();
    assert!(unsafe { GetPropW(window.0, w!("Bald.ClippedFrame")).0.is_null() });
    controller.restore_all_borders().unwrap();
    let mut frame = RECT::default();
    unsafe {
        GetWindowRect(window.0, &mut frame).unwrap();
    }
    assert!(window.client().2.y > frame.top);
}

#[test]
fn style_restore_requires_actual_caption_not_only_style_bits() {
    let window = Fixture::new(WS_OVERLAPPEDWINDOW);
    let controller = WindowController::default();
    controller.make_borderless(window.value()).unwrap();
    let before = window.client();
    unsafe {
        SetPropW(
            window.0,
            w!("Bald.TestRejectNativeCaption"),
            Some(HANDLE(1usize as *mut c_void)),
        )
        .unwrap();
    }
    assert!(
        controller
            .restore_all_borders()
            .unwrap_err()
            .to_string()
            .contains("native frame restoration unconfirmed after 5s")
    );
    assert_eq!(controller.managed_windows().len(), 1);
    let failed = window.client();
    assert_eq!(
        (failed.0, failed.1, failed.2.x, failed.2.y),
        (before.0, before.1, before.2.x, before.2.y)
    );
    unsafe {
        let _ = RemovePropW(window.0, w!("Bald.TestRejectNativeCaption"));
    }
    controller.restore_all_borders().unwrap();
    assert!(controller.managed_windows().is_empty());
}

#[test]
fn manual_drag_after_restart_does_not_require_or_adopt_frame_ownership() {
    let window = Fixture::new(WS_OVERLAPPEDWINDOW);
    let previous = WindowController::default();
    previous.make_borderless(window.value()).unwrap();
    let controller = WindowController::default();
    controller.configure_drag_rules(vec![(window.value(), DragMode::Disabled)]);
    assert!(!controller.has_drag_targets());
    controller.configure_drag_rules(vec![(window.value(), DragMode::Enabled)]);
    assert!(!controller.make_borderless(window.value()).unwrap());
    assert!(controller.has_drag_targets());
    assert_eq!(controller.drag_context(window.value()).unwrap().1, 16);
    let mut before = RECT::default();
    unsafe {
        GetWindowRect(window.0, &mut before).unwrap();
    }
    let cursor = POINT {
        x: before.left + 20,
        y: before.top + 8,
    };
    let mut drag = controller.begin_user_drag(window.value(), cursor).unwrap();
    controller
        .move_user_drag(
            &mut drag,
            POINT {
                x: cursor.x + 100,
                y: cursor.y + 100,
            },
        )
        .unwrap();
    assert!(controller.poll_user_drag(&mut drag).unwrap());
    let mut after = RECT::default();
    unsafe {
        GetWindowRect(window.0, &mut after).unwrap();
    }
    assert_eq!(
        (after.left, after.top),
        (before.left + 100, before.top + 100)
    );
    assert!(controller.original_windows.lock().unwrap().is_empty());
    controller.configure_drag_rules(vec![(window.value(), DragMode::Disabled)]);
    assert!(!controller.has_drag_targets());
    assert!(controller.begin_user_drag(window.value(), cursor).is_none());
    previous.restore_all_borders().unwrap();
}

#[test]
fn enumeration_skips_own_process_before_requesting_ui_title() {
    let window = Fixture::new(WS_OVERLAPPEDWINDOW);
    let mut found = Vec::<WindowInfo>::new();
    unsafe {
        let style = GetWindowLongW(window.0, GWL_EXSTYLE);
        set_window_long_checked(
            window.0,
            GWL_EXSTYLE,
            (style as u32 & !WS_EX_TOOLWINDOW.0) as i32,
        )
        .unwrap();
        let _ = RemovePropW(window.0, w!("Bald.TestTitleRequests"));
        assert!(enum_window(window.0, LPARAM(&mut found as *mut _ as isize)).as_bool());
        assert_eq!(
            GetPropW(window.0, w!("Bald.TestTitleRequests")).0 as usize,
            0
        );
    }
    assert!(found.is_empty());
}

#[test]
fn dragging_requires_explicit_permission_for_each_window() {
    let window = Fixture::new(WS_OVERLAPPEDWINDOW);
    let other = Fixture::new(WS_OVERLAPPEDWINDOW);
    let controller = WindowController::default();
    controller.configure_drag_rules(vec![
        (window.value(), DragMode::Disabled),
        (other.value(), DragMode::Disabled),
    ]);
    assert!(!controller.drag_allowed(window.value()));
    controller.set_window_drag_mode(window.value(), DragMode::Enabled);
    assert!(controller.drag_allowed(window.value()));
    assert!(!controller.drag_allowed(other.value()));
    controller.make_borderless(window.value()).unwrap();
    assert!(controller.has_drag_targets());
    controller.set_window_drag_mode(window.value(), DragMode::Disabled);
    assert!(!controller.has_drag_targets());
    assert!(
        controller
            .begin_user_drag(window.value(), POINT::default())
            .is_none()
    );
    controller.restore_all_borders().unwrap();
}

#[test]
fn blocked_borderless_strip_stays_intercepted_without_any_allowed_window() {
    let window = Fixture::new(WS_OVERLAPPEDWINDOW);
    let controller = WindowController::default();
    controller.configure_drag_rules(vec![(window.value(), DragMode::Disabled)]);
    assert!(!controller.has_strip_targets());
    controller.make_borderless(window.value()).unwrap();
    assert!(!controller.has_drag_targets());
    assert!(
        controller.has_strip_targets(),
        "blocking needs the hook even with no allowed app"
    );
    let mut before_outer = RECT::default();
    unsafe {
        GetWindowRect(window.0, &mut before_outer).unwrap();
    }
    let rect = controller.drag_surface(window.0).unwrap();
    for offset in [0, 5, 12, 15] {
        assert_eq!(
            controller.blocked_strip_target(
                window.value(),
                POINT {
                    x: rect.left + 100,
                    y: rect.top + offset
                }
            ),
            Some(window.value())
        );
    }
    for offset in [-1, 16, 20, 52] {
        assert_eq!(
            controller.blocked_strip_target(
                window.value(),
                POINT {
                    x: rect.left + 100,
                    y: rect.top + offset
                }
            ),
            None
        );
    }
    controller.set_window_drag_mode(window.value(), DragMode::Enabled);
    assert!(controller.has_strip_targets());
    assert_eq!(
        controller.blocked_strip_target(
            window.value(),
            POINT {
                x: rect.left + 100,
                y: rect.top + 5
            }
        ),
        None
    );
    let mut drag = controller
        .begin_user_drag(window.value(), POINT::default())
        .unwrap();
    controller.set_window_drag_mode(window.value(), DragMode::Disabled);
    assert!(
        !controller
            .move_user_drag(&mut drag, POINT { x: 200, y: 200 })
            .unwrap()
    );
    let mut after = RECT::default();
    unsafe {
        GetWindowRect(window.0, &mut after).unwrap();
    }
    assert_eq!(
        (after.left, after.top),
        (before_outer.left, before_outer.top)
    );
    controller.restore_all_borders().unwrap();
    assert!(!controller.has_strip_targets());
    assert_eq!(
        controller.blocked_strip_target(
            window.value(),
            POINT {
                x: rect.left + 100,
                y: rect.top + 5
            }
        ),
        None
    );
}

#[test]
fn native_region_preserves_styles_content_and_restores_each_cycle() {
    with_pumped_maple_fixture(|window| {
        let controller = WindowController::default();
        let style = unsafe { GetWindowLongW(window.0, GWL_STYLE) };
        let ex = unsafe { GetWindowLongW(window.0, GWL_EXSTYLE) };
        let mut client = RECT::default();
        unsafe {
            GetClientRect(window.0, &mut client).unwrap();
        }
        for _ in 0..3 {
            assert!(controller.clip_native_frame(window.value()).unwrap());
            assert!(is_borderless(window.value()));
            assert!(!controller.clip_native_frame(window.value()).unwrap());
            assert_eq!(unsafe { GetWindowLongW(window.0, GWL_STYLE) }, style);
            assert_eq!(unsafe { GetWindowLongW(window.0, GWL_EXSTYLE) }, ex);
            let surface = controller.drag_surface(window.0).unwrap();
            assert_eq!(
                (surface.right - surface.left, surface.bottom - surface.top),
                (client.right, client.bottom)
            );
            assert!(controller.restore_borders(window.value()).unwrap());
            assert!(!is_borderless(window.value()));
            let mut restored = RECT::default();
            unsafe {
                GetClientRect(window.0, &mut restored).unwrap();
            }
            assert_eq!(
                (restored.right, restored.bottom),
                (client.right, client.bottom)
            );
        }
    });
}

#[test]
fn maple_application_path_preserves_styles_and_restores_modern_rendering() {
    with_pumped_maple_fixture(|window| {
        let controller = WindowController::default();
        controller.configure_drag_rules(vec![(window.value(), DragMode::Enabled)]);
        let style = unsafe { GetWindowLongW(window.0, GWL_STYLE) };
        let ex = unsafe { GetWindowLongW(window.0, GWL_EXSTYLE) };
        let region = unsafe { CreateRectRgn(0, 0, 0, 0) };
        let before_region = unsafe { GetWindowRgn(window.0, region) };
        assert!(controller.make_borderless(window.value()).unwrap());
        assert_eq!(unsafe { GetWindowLongW(window.0, GWL_STYLE) }, style);
        assert_eq!(unsafe { GetWindowLongW(window.0, GWL_EXSTYLE) }, ex);
        assert_ne!(unsafe { GetWindowRgn(window.0, region) }, before_region);
        assert_eq!(native_frame_rendering(window.0), Some(false));
        assert!(controller.has_strip_targets());
        assert_eq!(controller.managed_windows().len(), 1);
        controller.restore_all_borders().unwrap();
        assert_eq!(native_frame_rendering(window.0), Some(true));
        let restored_region = unsafe { GetWindowRgn(window.0, region) };
        if restored_region != before_region {
            // Enabling DWM can install an equivalent full-window native region.
            // It must include the entire caption, never the prior client crop.
            let mut outer = RECT::default();
            unsafe {
                GetWindowRect(window.0, &mut outer).unwrap();
            }
            let full =
                unsafe { CreateRectRgn(0, 0, outer.right - outer.left, outer.bottom - outer.top) };
            assert!(unsafe { windows::Win32::Graphics::Gdi::EqualRgn(region, full).as_bool() });
            unsafe {
                let _ = DeleteObject(full.into());
            }
        }
        unsafe {
            let _ = DeleteObject(region.into());
        }
        assert!(controller.managed_windows().is_empty());
        assert!(!controller.has_strip_targets());
        assert!(
            controller
                .begin_user_drag(window.value(), POINT::default())
                .is_none()
        );
        controller.set_window_drag_mode(window.value(), DragMode::Disabled);
        assert!(!controller.center_blocked_window(window.value()).unwrap());
    });
}

fn with_pumped_maple_fixture(test: impl FnOnce(&Fixture)) {
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, MSG, PostThreadMessageW, WM_QUIT,
    };
    let previous_dpi =
        unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let (sender, receiver) = std::sync::mpsc::channel();
    let owner = std::thread::spawn(move || {
        let window = Fixture::new_fixture(WS_OVERLAPPEDWINDOW, false, true);
        sender
            .send((window.value(), unsafe {
                windows::Win32::System::Threading::GetCurrentThreadId()
            }))
            .unwrap();
        let mut message = MSG::default();
        while unsafe { GetMessageW(&mut message, None, 0, 0).0 } > 0 {
            unsafe {
                DispatchMessageW(&message);
            }
        }
    });
    let (value, thread_id) = receiver.recv().unwrap();
    // Like the real game, the owner must keep processing DWM notifications
    // while the controller waits. Only the owner destroys this window.
    let view = std::mem::ManuallyDrop::new(Fixture(HWND(value as *mut c_void)));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| test(&view)));
    unsafe {
        PostThreadMessageW(thread_id, WM_QUIT, WPARAM(0), LPARAM(0)).unwrap();
    }
    owner.join().unwrap();
    unsafe {
        SetThreadDpiAwarenessContext(previous_dpi);
    }
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}

#[test]
fn maple_restoration_preserves_existing_custom_region_and_classic_rendering() {
    with_pumped_maple_fixture(|window| {
        let custom = unsafe { CreateRectRgn(0, 0, 790, 590) };
        unsafe {
            DwmSetWindowAttribute(
                window.0,
                DWMWA_NCRENDERING_POLICY,
                &DWMNCRP_DISABLED as *const _ as *const c_void,
                std::mem::size_of_val(&DWMNCRP_DISABLED) as u32,
            )
            .unwrap();
            SetWindowPos(
                window.0,
                None,
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | QUIET_POSITION,
            )
            .unwrap();
            assert_ne!(SetWindowRgn(window.0, Some(custom), false), 0);
        }
        let controller = WindowController::default();
        controller.make_borderless(window.value()).unwrap();
        controller.restore_all_borders().unwrap();
        let restored = unsafe { CreateRectRgn(0, 0, 0, 0) };
        let expected = unsafe { CreateRectRgn(0, 0, 790, 590) };
        unsafe {
            assert_ne!(GetWindowRgn(window.0, restored).0, 0);
            assert!(windows::Win32::Graphics::Gdi::EqualRgn(restored, expected).as_bool());
            let _ = DeleteObject(restored.into());
            let _ = DeleteObject(expected.into());
        }
        assert_eq!(native_frame_rendering(window.0), Some(false));
    });
}

#[test]
fn blocking_centers_once_without_resize_focus_or_visibility_changes() {
    let window = Fixture::new(WS_POPUP);
    let controller = WindowController::default();
    controller.configure_drag_rules(vec![(window.value(), DragMode::Enabled)]);
    let mut before = RECT::default();
    unsafe {
        GetWindowRect(window.0, &mut before).unwrap();
    }
    let foreground = unsafe { GetForegroundWindow() };
    let bounds = RECT {
        left: -26000,
        top: -26000,
        right: -22000,
        bottom: -22000,
    };
    let pid = window_process_id(window.0);
    assert!(
        controller
            .center_blocked_in_rect(window.value(), pid, bounds)
            .is_err()
    );
    controller.set_window_drag_mode(window.value(), DragMode::Disabled);
    assert!(
        controller
            .center_blocked_in_rect(window.value(), pid, bounds)
            .unwrap()
    );
    let mut actual = RECT::default();
    unsafe {
        GetWindowRect(window.0, &mut actual).unwrap();
    }
    assert_eq!((actual.left, actual.top), (-24400, -24300));
    assert_eq!(
        (actual.right - actual.left, actual.bottom - actual.top),
        (before.right - before.left, before.bottom - before.top)
    );
    assert_eq!(unsafe { GetForegroundWindow() }, foreground);
    assert!(unsafe { IsWindowVisible(window.0).as_bool() });
    assert!(!controller.drag_allowed(window.value()));
    assert!(
        controller
            .begin_user_drag(window.value(), POINT::default())
            .is_none()
    );
    assert!(
        !controller
            .center_blocked_in_rect(window.value(), pid, bounds)
            .unwrap()
    );
    assert!(controller.original_windows.lock().unwrap().is_empty());

    unsafe {
        let _ = ShowWindow(window.0, SW_SHOWMINNOACTIVE);
    }
    assert!(
        !controller
            .center_blocked_in_rect(window.value(), pid, bounds)
            .unwrap()
    );
    assert!(unsafe { IsIconic(window.0).as_bool() });
    unsafe {
        let _ = ShowWindow(window.0, SW_HIDE);
    }
    assert!(
        !controller
            .center_blocked_in_rect(window.value(), pid, bounds)
            .unwrap()
    );
    assert!(!unsafe { IsWindowVisible(window.0).as_bool() });
}

#[test]
fn centering_rejects_pending_drag_and_stale_process_identity() {
    let window = Fixture::new(WS_POPUP);
    let controller = WindowController::default();
    controller.configure_drag_rules(vec![(window.value(), DragMode::Disabled)]);
    let bounds = RECT {
        left: -26000,
        top: -26000,
        right: -22000,
        bottom: -22000,
    };
    let pid = window_process_id(window.0);
    assert!(
        controller
            .center_blocked_in_rect(window.value(), u32::MAX, bounds)
            .is_err()
    );
    controller
        .drag_rules
        .lock()
        .unwrap()
        .get_mut(&window.value())
        .unwrap()
        .in_flight = true;
    assert!(
        controller
            .center_blocked_in_rect(window.value(), pid, bounds)
            .is_err()
    );
    let mut actual = RECT::default();
    unsafe {
        GetWindowRect(window.0, &mut actual).unwrap();
    }
    assert_eq!((actual.left, actual.top), (-24000, -24000));
}

#[test]
fn maple_drag_does_not_allow_position_callback_to_pin_y_to_zero() {
    let window = Fixture::new_fixture(WS_OVERLAPPEDWINDOW, false, true);
    let controller = WindowController::default();
    controller.configure_drag_rules(vec![(window.value(), DragMode::Enabled)]);
    controller.clip_native_frame(window.value()).unwrap();
    let surface = controller.drag_surface(window.0).unwrap();
    let mut before = RECT::default();
    unsafe {
        GetWindowRect(window.0, &mut before).unwrap();
        SetPropW(
            window.0,
            w!("Bald.TestClampPosition"),
            Some(HANDLE(std::ptr::dangling_mut())),
        )
        .unwrap();
    }
    let cursor = POINT {
        x: surface.left + 20,
        y: surface.top + 8,
    };
    let mut drag = controller.begin_user_drag(window.value(), cursor).unwrap();
    for (dx, dy) in [(100, 200), (-100, -200), (300, 600)] {
        assert!(
            controller
                .move_user_drag(
                    &mut drag,
                    POINT {
                        x: cursor.x + dx,
                        y: cursor.y + dy
                    }
                )
                .unwrap()
        );
        let mut actual = RECT::default();
        unsafe {
            GetWindowRect(window.0, &mut actual).unwrap();
        }
        assert_eq!(
            (actual.left, actual.top),
            (before.left + dx, before.top + dy)
        );
        assert!(controller.poll_user_drag(&mut drag).unwrap());
        assert!(!WindowController::drag_move_pending(&drag));
    }
    controller.set_window_drag_mode(window.value(), DragMode::Disabled);
    let bounds = RECT {
        left: -26000,
        top: -26000,
        right: -22000,
        bottom: -22000,
    };
    assert!(
        controller
            .center_blocked_in_rect(window.value(), window_process_id(window.0), bounds)
            .unwrap()
    );
    let mut centered = RECT::default();
    unsafe {
        GetWindowRect(window.0, &mut centered).unwrap();
    }
    assert_eq!(
        (centered.left, centered.top),
        (
            bounds.left + (4000 - (surface.right - surface.left)) / 2
                - (surface.left - before.left),
            bounds.top + (4000 - (surface.bottom - surface.top)) / 2 - (surface.top - before.top)
        )
    );
    unsafe {
        let _ = RemovePropW(window.0, w!("Bald.TestClampPosition"));
    }
    controller.restore_all_borders().unwrap();
}

#[test]
fn owner_position_constraint_is_not_bypassed_and_stalls_only_its_own_drag() {
    let window = Fixture::new(WS_OVERLAPPEDWINDOW);
    let other = Fixture::new(WS_OVERLAPPEDWINDOW);
    let controller = WindowController::default();
    controller.configure_drag_rules(vec![
        (window.value(), DragMode::Enabled),
        (other.value(), DragMode::Enabled),
    ]);
    controller.make_borderless(window.value()).unwrap();
    let mut before = RECT::default();
    unsafe {
        GetWindowRect(window.0, &mut before).unwrap();
        SetPropW(
            window.0,
            w!("Bald.TestClampPosition"),
            Some(HANDLE(std::ptr::dangling_mut())),
        )
        .unwrap();
    }
    let cursor = POINT {
        x: before.left + 20,
        y: before.top + 8,
    };
    let mut drag = controller.begin_user_drag(window.value(), cursor).unwrap();
    let first = POINT {
        x: cursor.x + 100,
        y: cursor.y + 100,
    };
    controller.move_user_drag(&mut drag, first).unwrap();
    // Keep the burst test independent of machine speed; timeout is checked below.
    drag.pending.as_mut().unwrap().started =
        std::time::Instant::now() + std::time::Duration::from_secs(10);
    let expected = drag.pending.unwrap();
    for index in 0..1000 {
        assert!(
            controller
                .move_user_drag(
                    &mut drag,
                    POINT {
                        x: first.x + index,
                        y: first.y + index
                    }
                )
                .unwrap()
        );
        let pending = drag.pending.unwrap();
        assert_eq!(
            (pending.x, pending.y, pending.started),
            (expected.x, expected.y, expected.started)
        );
    }
    let mut actual = RECT::default();
    unsafe {
        GetWindowRect(window.0, &mut actual).unwrap();
    }
    assert_eq!(actual.top, 0);
    drag.pending.as_mut().unwrap().started =
        std::time::Instant::now() - std::time::Duration::from_millis(101);
    assert!(!controller.poll_user_drag(&mut drag).unwrap());
    assert!(!controller.drag_allowed(window.value()));
    assert!(controller.drag_allowed(other.value()));
    assert!(controller.begin_user_drag(window.value(), cursor).is_none());
    unsafe {
        let _ = RemovePropW(window.0, w!("Bald.TestClampPosition"));
    }
    controller.restore_all_borders().unwrap();
}

#[test]
fn direct_drag_uses_fixed_anchor_reaches_top_and_stops_after_restore() {
    let window = Fixture::new(WS_OVERLAPPEDWINDOW);
    let controller = WindowController::default();
    controller.configure_drag_rules(vec![(window.value(), DragMode::Enabled)]);
    controller.make_borderless(window.value()).unwrap();
    let mut before = RECT::default();
    unsafe {
        GetWindowRect(window.0, &mut before).unwrap();
    }
    let cursor = POINT {
        x: before.left + 20,
        y: before.top + 8,
    };
    let mut drag = controller.begin_user_drag(window.value(), cursor).unwrap();
    for (dx, dy) in [(300, 500), (-400, -200), (0, -before.top)] {
        assert!(
            controller
                .move_user_drag(
                    &mut drag,
                    POINT {
                        x: cursor.x + dx,
                        y: cursor.y + dy
                    }
                )
                .unwrap()
        );
        assert!(controller.poll_user_drag(&mut drag).unwrap());
        let mut moved = RECT::default();
        unsafe {
            GetWindowRect(window.0, &mut moved).unwrap();
        }
        assert_eq!((moved.left, moved.top), (before.left + dx, before.top + dy));
        assert_eq!(
            (moved.right - moved.left, moved.bottom - moved.top),
            (before.right - before.left, before.bottom - before.top)
        );
    }
    controller.restore_all_borders().unwrap();
    let mut restored = RECT::default();
    unsafe {
        GetWindowRect(window.0, &mut restored).unwrap();
    }
    assert!(!controller.move_user_drag(&mut drag, cursor).unwrap());
    let mut unchanged = RECT::default();
    unsafe {
        GetWindowRect(window.0, &mut unchanged).unwrap();
    }
    assert_eq!(restored, unchanged);
}

#[test]
fn managed_hidden_untitled_window_is_still_restored() {
    let window = Fixture::new(WS_OVERLAPPEDWINDOW);
    let controller = WindowController::default();
    controller.configure_drag_rules(vec![(window.value(), DragMode::Enabled)]);
    assert!(!controller.has_drag_targets());
    controller.make_borderless(window.value()).unwrap();
    assert!(controller.has_drag_targets());
    unsafe {
        SetWindowTextW(window.0, w!("")).unwrap();
        let _ = ShowWindow(window.0, SW_HIDE);
    }
    assert_eq!(controller.managed_windows().len(), 1);
    controller.restore_all_borders().unwrap();
    assert_eq!(
        unsafe { GetWindowLongW(window.0, GWL_STYLE) } as u32 & BORDER_STYLES,
        BORDER_STYLES
    );
    assert!(!unsafe { IsWindowVisible(window.0).as_bool() });
    assert!(controller.managed_windows().is_empty());
    assert!(!controller.has_drag_targets());
}

#[test]
fn restoring_minimized_window_preserves_minimization_and_restore_size() {
    let window = Fixture::new(WS_OVERLAPPEDWINDOW);
    let controller = WindowController::default();
    controller.make_borderless(window.value()).unwrap();
    unsafe {
        SetWindowPos(
            window.0,
            None,
            -23000,
            -22000,
            900,
            700,
            SWP_NOACTIVATE | SWP_NOZORDER,
        )
        .unwrap();
        let _ = ShowWindow(window.0, SW_SHOWMINNOACTIVE);
    }
    controller.restore_borders(window.value()).unwrap();
    assert!(unsafe { IsIconic(window.0).as_bool() });
    assert!(unsafe { IsWindowVisible(window.0).as_bool() });
    unsafe {
        let _ = ShowWindow(window.0, SW_SHOWNOACTIVATE);
    }
    let (width, height, _) = window.client();
    assert_eq!((width, height), (900, 700));
}

#[test]
fn stale_process_identity_cannot_restore_a_different_window() {
    let window = Fixture::new(WS_OVERLAPPEDWINDOW);
    let controller = WindowController::default();
    controller.make_borderless(window.value()).unwrap();
    controller
        .original_windows
        .lock()
        .unwrap()
        .get_mut(&window.value())
        .unwrap()
        .process_id = u32::MAX;
    let before = unsafe { GetWindowLongW(window.0, GWL_STYLE) };
    assert!(!controller.restore_borders(window.value()).unwrap());
    assert_eq!(unsafe { GetWindowLongW(window.0, GWL_STYLE) }, before);
    assert!(controller.managed_windows().is_empty());
}

#[test]
fn clipping_keeps_current_content_size_and_preserves_native_style() {
    let window = Fixture::new(WS_OVERLAPPEDWINDOW);
    let controller = WindowController::default();
    let (width, height, _) = window.client();
    let style = unsafe { GetWindowLongW(window.0, GWL_STYLE) };
    assert!(controller.clip_game_frame(window.value()).unwrap());
    let (after_width, after_height, _) = window.client();
    assert_eq!((after_width, after_height), (width, height));
    assert_eq!(unsafe { GetWindowLongW(window.0, GWL_STYLE) }, style);
    controller.restore_all_borders().unwrap();
    assert_eq!(unsafe { GetWindowLongW(window.0, GWL_STYLE) }, style);
}

#[test]
fn region_sensitive_game_restoration_preserves_render_size_and_origin() {
    let window = Fixture::new_kind(WS_OVERLAPPEDWINDOW, true);
    let controller = WindowController::default();
    let before = window.client();
    controller.clip_game_frame(window.value()).unwrap();
    let clipped = window.client();
    assert_eq!((clipped.0, clipped.1), (before.0, before.1));
    controller.restore_all_borders().unwrap();
    let after = window.client();
    assert_eq!((after.0, after.1), (before.0, before.1));
    assert_eq!((after.2.x, after.2.y), (clipped.2.x, clipped.2.y));
}

#[test]
fn region_sensitive_resolution_changes_do_not_crop_or_expand_content() {
    let window = Fixture::new_kind(WS_OVERLAPPEDWINDOW, true);
    let controller = WindowController::default();
    let mut outer = RECT::default();
    unsafe {
        GetWindowRect(window.0, &mut outer).unwrap();
    }
    let before = window.client();
    let width_extra = outer.right - outer.left - before.0;
    let height_extra = outer.bottom - outer.top - before.1;
    controller.clip_game_frame(window.value()).unwrap();
    for (width, height) in [(1366, 768), (1920, 1080), (2560, 1440)] {
        unsafe {
            // Simulate the game restoring native geometry on resolution change.
            assert_ne!(SetWindowRgn(window.0, None, false), 0);
            SetWindowPos(
                window.0,
                None,
                -24000,
                -24000,
                width + width_extra,
                height + height_extra,
                SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOZORDER,
            )
            .unwrap();
        }
        assert_eq!((window.client().0, window.client().1), (width, height));
        controller.clip_game_frame(window.value()).unwrap();
        assert_eq!((window.client().0, window.client().1), (width, height));
    }
    controller.restore_all_borders().unwrap();
    assert_eq!((window.client().0, window.client().1), (2560, 1440));
    assert!(unsafe { IsWindowVisible(window.0).as_bool() });
}

#[test]
fn shutdown_restores_frames_and_prevents_late_reapplication() {
    let window = Fixture::new(WS_OVERLAPPEDWINDOW);
    let controller = WindowController::default();
    controller.make_borderless(window.value()).unwrap();
    assert!(controller.begin_shutdown());
    controller.restore_all_borders().unwrap();
    assert!(!controller.make_borderless(window.value()).unwrap());
    assert_eq!(
        unsafe { GetWindowLongW(window.0, GWL_STYLE) } as u32 & BORDER_STYLES,
        BORDER_STYLES
    );
}

#[test]
fn clipped_minimized_game_keeps_visibility_and_restore_content_size() {
    let window = Fixture::new_kind(WS_OVERLAPPEDWINDOW, true);
    let controller = WindowController::default();
    let before = window.client();
    controller.clip_game_frame(window.value()).unwrap();
    unsafe {
        let _ = ShowWindow(window.0, SW_SHOWMINNOACTIVE);
    }
    controller.restore_all_borders().unwrap();
    assert!(unsafe { IsWindowVisible(window.0).as_bool() });
    assert!(unsafe { IsIconic(window.0).as_bool() });
    unsafe {
        let _ = ShowWindow(window.0, SW_SHOWNOACTIVATE);
    }
    let restored = window.client();
    assert_eq!((restored.0, restored.1), (before.0, before.1));
}
