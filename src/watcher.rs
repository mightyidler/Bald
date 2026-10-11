use std::{
    collections::HashMap,
    sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    sync::{Arc, Mutex, OnceLock, RwLock, mpsc},
    thread,
};

use uuid::Uuid;
use windows::Win32::{
    Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM},
    System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
    UI::{
        Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent},
        WindowsAndMessaging::{
            CallNextHookEx, DispatchMessageW, EVENT_OBJECT_CREATE, EVENT_OBJECT_HIDE,
            EVENT_OBJECT_LOCATIONCHANGE, EVENT_SYSTEM_MINIMIZEEND, EVENT_SYSTEM_MINIMIZESTART,
            EVENT_SYSTEM_MOVESIZEEND, GetMessageW, KillTimer, MSG, MSLLHOOKSTRUCT, OBJID_WINDOW,
            PM_NOREMOVE, PeekMessageW, PostThreadMessageW, SetTimer, SetWindowsHookExW,
            TranslateMessage, UnhookWindowsHookEx, WH_MOUSE_LL, WINEVENT_OUTOFCONTEXT,
            WINEVENT_SKIPOWNPROCESS, WM_APP, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_QUIT,
            WM_TIMER,
        },
    },
};

use crate::{
    config::Config,
    window_manager::{WindowController, enumerate_windows, is_borderless},
};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RuleStatus {
    NotRunning,
    Running,
    Borderless,
    Failed(String),
}

pub struct Watcher {
    wake: mpsc::Sender<()>,
    stop: mpsc::Sender<()>,
    thread: Option<thread::JoinHandle<()>>,
    scan_thread_id: u32,
    mouse_thread_id: u32,
    mouse_thread: Option<thread::JoinHandle<()>>,
    drag_requests: mpsc::Sender<DragEvent>,
    drag_thread: Option<thread::JoinHandle<()>>,
}

static WINDOW_EVENT_WAKE: OnceLock<Mutex<Option<mpsc::Sender<()>>>> = OnceLock::new();
static TRACKED_WINDOWS: OnceLock<Mutex<HashMap<isize, u32>>> = OnceLock::new();
static MOUSE_CONTROLLER: OnceLock<Mutex<Option<WindowController>>> = OnceLock::new();
#[derive(Clone, Copy)]
enum DragEvent {
    Begin(isize, POINT),
    Move(u32),
    End(POINT),
    Cancel,
    Stop,
}
static MOUSE_DRAG_REQUESTS: OnceLock<Mutex<Option<mpsc::Sender<DragEvent>>>> = OnceLock::new();
static USER_DRAG_ACTIVE: AtomicBool = AtomicBool::new(false);
static BLOCKED_STRIP_PRESS: AtomicBool = AtomicBool::new(false);
struct DragMailbox {
    point: AtomicU64,
    queued: AtomicBool,
}
impl DragMailbox {
    const fn new() -> Self {
        Self {
            point: AtomicU64::new(0),
            queued: AtomicBool::new(false),
        }
    }
    fn publish(&self, point: POINT) -> bool {
        self.point.store(pack_point(point), Ordering::Release);
        !self.queued.swap(true, Ordering::AcqRel)
    }
    fn consume(&self) -> POINT {
        self.queued.store(false, Ordering::Release);
        unpack_point(self.point.load(Ordering::Acquire))
    }
}
static DRAG_MAILBOX: DragMailbox = DragMailbox::new();
static DRAG_GENERATION: AtomicU32 = AtomicU32::new(0);

fn pack_point(point: POINT) -> u64 {
    point.x as u32 as u64 | ((point.y as u32 as u64) << 32)
}
fn unpack_point(value: u64) -> POINT {
    POINT {
        x: value as u32 as i32,
        y: (value >> 32) as u32 as i32,
    }
}
static SCAN_THREAD_ID: AtomicU32 = AtomicU32::new(0);
static MOUSE_THREAD_ID: AtomicU32 = AtomicU32::new(0);
static SCAN_WAKE_PENDING: AtomicBool = AtomicBool::new(false);
const WM_SCAN_REQUEST: u32 = WM_APP + 17;
const WM_SET_DRAG_HOOK: u32 = WM_APP + 18;

fn update_drag_hook(enabled: bool) {
    let thread_id = MOUSE_THREAD_ID.load(Ordering::Acquire);
    if thread_id != 0 {
        unsafe {
            let _ = PostThreadMessageW(
                thread_id,
                WM_SET_DRAG_HOOK,
                WPARAM(enabled as usize),
                LPARAM(0),
            );
        }
    }
}

fn request_scan(sender: &mpsc::Sender<()>) {
    if enqueue_scan(&SCAN_WAKE_PENDING, sender) {
        unsafe {
            let _ = PostThreadMessageW(
                SCAN_THREAD_ID.load(Ordering::Acquire),
                WM_SCAN_REQUEST,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

fn enqueue_scan(pending: &AtomicBool, sender: &mpsc::Sender<()>) -> bool {
    if pending.swap(true, Ordering::AcqRel) {
        return false;
    }
    if sender.send(()).is_err() {
        pending.store(false, Ordering::Release);
        return false;
    }
    true
}

unsafe extern "system" fn window_event_callback(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    object_id: i32,
    child_id: i32,
    _thread_id: u32,
    _event_time: u32,
) {
    if hwnd.0.is_null() || object_id != OBJID_WINDOW.0 || child_id != 0 {
        return;
    }
    // Out-of-context callbacks only request a scan; window operations stay on the scanner.
    if event == EVENT_OBJECT_LOCATIONCHANGE
        && (USER_DRAG_ACTIVE.load(Ordering::Acquire)
            || !TRACKED_WINDOWS
                .get()
                .is_some_and(|tracked| tracked.lock().unwrap().contains_key(&(hwnd.0 as isize))))
    {
        return;
    }
    if let Some(sender) = WINDOW_EVENT_WAKE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap()
        .as_ref()
    {
        request_scan(sender);
    }
}

unsafe extern "system" fn mouse_hook_callback(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code < 0 {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }
    let message = wparam.0 as u32;
    if message != WM_LBUTTONDOWN
        && !USER_DRAG_ACTIVE.load(Ordering::Acquire)
        && !(message == WM_LBUTTONUP && BLOCKED_STRIP_PRESS.load(Ordering::Acquire))
    {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }
    let point = unsafe { (*(lparam.0 as *const MSLLHOOKSTRUCT)).pt };
    let Some(sender_guard) = MOUSE_DRAG_REQUESTS
        .get()
        .and_then(|stored| stored.try_lock().ok())
    else {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    };
    let Some(sender) = sender_guard.as_ref() else {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    };
    let controller_guard = MOUSE_CONTROLLER
        .get()
        .and_then(|stored| stored.try_lock().ok());
    let controller = controller_guard.as_ref().and_then(|stored| stored.as_ref());
    if message == WM_LBUTTONDOWN {
        BLOCKED_STRIP_PRESS.store(false, Ordering::Release);
        // A secure-desktop/focus transition can hide the previous button-up.
        // A new press must not inherit that drag or consume an unrelated release.
        if USER_DRAG_ACTIVE.swap(false, Ordering::AcqRel) {
            let _ = sender.send(DragEvent::Cancel);
        }
        if controller
            .and_then(|controller| controller.native_blocked_drag_target(point))
            .is_some()
        {
            BLOCKED_STRIP_PRESS.store(true, Ordering::Release);
            return LRESULT(1);
        }
        let target = controller.and_then(|controller| controller.native_drag_target(point));
        if let Some(hwnd) = target
            && sender.send(DragEvent::Begin(hwnd, point)).is_ok()
        {
            DRAG_GENERATION.fetch_add(1, Ordering::AcqRel);
            DRAG_MAILBOX
                .point
                .store(pack_point(point), Ordering::Release);
            USER_DRAG_ACTIVE.store(true, Ordering::Release);
            // Consume only this managed window's draggable-strip click,
            // preventing a simultaneous game/native move loop.
            return LRESULT(1);
        }
    } else if message == WM_LBUTTONUP && BLOCKED_STRIP_PRESS.swap(false, Ordering::AcqRel) {
        return LRESULT(1);
    } else if message == WM_MOUSEMOVE {
        if DRAG_MAILBOX.publish(point)
            && sender
                .send(DragEvent::Move(DRAG_GENERATION.load(Ordering::Acquire)))
                .is_err()
        {
            DRAG_MAILBOX.queued.store(false, Ordering::Release);
        }
    } else if message == WM_LBUTTONUP && USER_DRAG_ACTIVE.swap(false, Ordering::AcqRel) {
        let _ = sender.send(DragEvent::End(point));
        return LRESULT(1);
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

impl Watcher {
    #[cfg(test)]
    pub fn start(
        config: Arc<RwLock<Config>>,
        statuses: Arc<RwLock<HashMap<Uuid, RuleStatus>>>,
        controller: WindowController,
    ) -> Self {
        Self::start_with_elevation(config, statuses, controller, None)
    }

    pub fn start_with_elevation(
        config: Arc<RwLock<Config>>,
        statuses: Arc<RwLock<HashMap<Uuid, RuleStatus>>>,
        controller: WindowController,
        elevation: Option<mpsc::SyncSender<bool>>,
    ) -> Self {
        let (wake_tx, wake_rx) = mpsc::channel();
        let (stop_tx, stop_rx) = mpsc::channel();
        let (mouse_ready_tx, mouse_ready_rx) = mpsc::sync_channel(1);
        let (scan_ready_tx, scan_ready_rx) = mpsc::sync_channel(1);
        let (drag_tx, drag_rx) = mpsc::channel::<DragEvent>();
        let drag_controller = controller.clone();
        *MOUSE_DRAG_REQUESTS
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap() = Some(drag_tx.clone());
        SCAN_WAKE_PENDING.store(false, Ordering::Release);
        DRAG_MAILBOX.queued.store(false, Ordering::Release);
        let drag_thread = thread::spawn(move || {
            let mut gesture = None;
            let mut latest = POINT::default();
            let mut ending = false;
            let mut cancelled = false;
            loop {
                let event = if gesture
                    .as_ref()
                    .is_some_and(|drag| ending || WindowController::drag_move_pending(drag))
                {
                    match drag_rx.recv_timeout(std::time::Duration::from_millis(16)) {
                        Ok(event) => Some(event),
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => None,
                    }
                } else {
                    match drag_rx.recv() {
                        Ok(event) => Some(event),
                        Err(_) => break,
                    }
                };
                let Some(event) = event else {
                    if let Some(drag) = gesture.as_mut() {
                        let active = if cancelled {
                            drag_controller.poll_user_drag(drag)
                        } else if ending {
                            drag_controller.finish_user_drag(drag, latest)
                        } else {
                            drag_controller.move_user_drag(drag, latest)
                        };
                        if !active.unwrap_or(false)
                            || (cancelled && !WindowController::drag_move_pending(drag))
                        {
                            gesture = None;
                        }
                    }
                    continue;
                };
                match event {
                    DragEvent::Begin(hwnd, point) => {
                        // A previous request must finish before another gesture can issue one.
                        if gesture
                            .as_ref()
                            .is_some_and(WindowController::drag_move_pending)
                        {
                            continue;
                        }
                        latest = point;
                        ending = false;
                        cancelled = false;
                        gesture = drag_controller.begin_user_drag(hwnd, point);
                    }
                    DragEvent::Move(generation) => {
                        let point = DRAG_MAILBOX.consume();
                        if generation != DRAG_GENERATION.load(Ordering::Acquire) || ending {
                            continue;
                        }
                        latest = point;
                        if let Some(drag) = gesture.as_mut()
                            && !drag_controller
                                .move_user_drag(drag, latest)
                                .unwrap_or(false)
                        {
                            gesture = None;
                        }
                    }
                    DragEvent::End(point) => {
                        latest = point;
                        ending = true;
                        if let Some(drag) = gesture.as_mut()
                            && !drag_controller
                                .finish_user_drag(drag, latest)
                                .unwrap_or(false)
                        {
                            gesture = None;
                        }
                    }
                    DragEvent::Stop => break,
                    DragEvent::Cancel => {
                        ending = true;
                        cancelled = true;
                        if !gesture
                            .as_ref()
                            .is_some_and(WindowController::drag_move_pending)
                        {
                            gesture = None;
                        }
                    }
                }
            }
        });
        *WINDOW_EVENT_WAKE
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap() = Some(wake_tx.clone());
        *MOUSE_CONTROLLER
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap() = Some(controller.clone());
        let mouse_thread = thread::spawn(move || {
            let thread_id = unsafe { GetCurrentThreadId() };
            let module = unsafe {
                GetModuleHandleW(None)
                    .ok()
                    .map(|module| HINSTANCE(module.0))
            };
            let mut message = MSG::default();
            unsafe {
                let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
            }
            MOUSE_THREAD_ID.store(thread_id, Ordering::Release);
            let mut mouse_hook = None;
            let _ = mouse_ready_tx.send(thread_id);
            unsafe {
                while GetMessageW(&mut message, None, 0, 0).0 > 0 {
                    if message.message == WM_SET_DRAG_HOOK {
                        if message.wParam.0 != 0 && mouse_hook.is_none() {
                            mouse_hook = SetWindowsHookExW(
                                WH_MOUSE_LL,
                                Some(mouse_hook_callback),
                                module,
                                0,
                            )
                            .ok();
                        } else if message.wParam.0 == 0
                            && let Some(hook) = mouse_hook.take()
                        {
                            USER_DRAG_ACTIVE.store(false, Ordering::Release);
                            BLOCKED_STRIP_PRESS.store(false, Ordering::Release);
                            if let Some(sender) = MOUSE_DRAG_REQUESTS
                                .get()
                                .and_then(|stored| stored.lock().ok())
                                .as_deref()
                                .and_then(|stored| stored.as_ref())
                            {
                                let _ = sender.send(DragEvent::Cancel);
                            }
                            let _ = UnhookWindowsHookEx(hook);
                        }
                    } else {
                        let _ = TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                }
                if let Some(hook) = mouse_hook {
                    let _ = UnhookWindowsHookEx(hook);
                }
            }
            MOUSE_THREAD_ID.store(0, Ordering::Release);
            USER_DRAG_ACTIVE.store(false, Ordering::Release);
            BLOCKED_STRIP_PRESS.store(false, Ordering::Release);
        });
        let mouse_thread_id = mouse_ready_rx.recv().unwrap_or(0);
        let thread = thread::spawn(move || {
            let thread_id = unsafe { GetCurrentThreadId() };
            let mut message = MSG::default();
            // Create the message queue before accepting cross-thread wakeups.
            unsafe {
                let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
            }
            SCAN_THREAD_ID.store(thread_id, Ordering::Release);
            let hook = unsafe {
                SetWinEventHook(
                    EVENT_OBJECT_CREATE,
                    EVENT_OBJECT_HIDE,
                    None,
                    Some(window_event_callback),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
                )
            };
            let move_end_hook = unsafe {
                SetWinEventHook(
                    EVENT_SYSTEM_MOVESIZEEND,
                    EVENT_SYSTEM_MOVESIZEEND,
                    None,
                    Some(window_event_callback),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
                )
            };
            let size_hook = unsafe {
                SetWinEventHook(
                    EVENT_OBJECT_LOCATIONCHANGE,
                    EVENT_OBJECT_LOCATIONCHANGE,
                    None,
                    Some(window_event_callback),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
                )
            };
            let minimize_hook = unsafe {
                SetWinEventHook(
                    EVENT_SYSTEM_MINIMIZESTART,
                    EVENT_SYSTEM_MINIMIZEEND,
                    None,
                    Some(window_event_callback),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
                )
            };
            let timer = unsafe { SetTimer(None, 0, 4000, None) };
            let _ = scan_ready_tx.send(thread_id);
            let mut needs_elevation = false;
            notify_elevation(
                &mut needs_elevation,
                scan(&config, &statuses, &controller),
                elevation.as_ref(),
            );
            // GetMessage blocks in Windows. Idle operation has no 16ms polling.
            // Event bursts are coalesced into one pending scan request.
            while unsafe { GetMessageW(&mut message, None, 0, 0).0 } > 0 {
                if stop_rx.try_recv().is_ok() {
                    break;
                }
                if message.message == WM_SCAN_REQUEST || message.message == WM_TIMER {
                    SCAN_WAKE_PENDING.store(false, Ordering::Release);
                    while wake_rx.try_recv().is_ok() {}
                    notify_elevation(
                        &mut needs_elevation,
                        scan(&config, &statuses, &controller),
                        elevation.as_ref(),
                    );
                } else {
                    unsafe {
                        let _ = TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                }
            }
            if timer != 0 {
                unsafe {
                    let _ = KillTimer(None, timer);
                }
            }
            SCAN_THREAD_ID.store(0, Ordering::Release);
            SCAN_WAKE_PENDING.store(false, Ordering::Release);
            if !hook.0.is_null() {
                unsafe {
                    let _ = UnhookWinEvent(hook);
                }
            }
            if !size_hook.0.is_null() {
                unsafe {
                    let _ = UnhookWinEvent(size_hook);
                }
            }
            if !minimize_hook.0.is_null() {
                unsafe {
                    let _ = UnhookWinEvent(minimize_hook);
                }
            }
            if !move_end_hook.0.is_null() {
                unsafe {
                    let _ = UnhookWinEvent(move_end_hook);
                }
            }
        });
        let scan_thread_id = scan_ready_rx.recv().unwrap_or(0);
        Self {
            wake: wake_tx,
            stop: stop_tx,
            thread: Some(thread),
            scan_thread_id,
            mouse_thread_id,
            mouse_thread: Some(mouse_thread),
            drag_requests: drag_tx,
            drag_thread: Some(drag_thread),
        }
    }

    pub fn notify(&self) {
        request_scan(&self.wake);
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if self.scan_thread_id != 0 {
            unsafe {
                let _ = PostThreadMessageW(self.scan_thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
            }
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        if self.mouse_thread_id != 0 {
            unsafe {
                let _ = PostThreadMessageW(self.mouse_thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
            }
        }
        if let Some(thread) = self.mouse_thread.take() {
            let _ = thread.join();
        }
        let _ = self.drag_requests.send(DragEvent::Stop);
        if let Some(thread) = self.drag_thread.take() {
            let _ = thread.join();
        }
        *MOUSE_CONTROLLER
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap() = None;
        *MOUSE_DRAG_REQUESTS
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap() = None;
        *WINDOW_EVENT_WAKE
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap() = None;
        TRACKED_WINDOWS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap()
            .clear();
        SCAN_WAKE_PENDING.store(false, Ordering::Release);
    }
}

fn scan(
    config: &Arc<RwLock<Config>>,
    statuses: &Arc<RwLock<HashMap<Uuid, RuleStatus>>>,
    controller: &WindowController,
) -> bool {
    // Hold the read guard for the complete scan. Configuration mutations then wait
    // for an in-flight apply to finish before disabling a rule and restoring frames.
    let snapshot = config.read().unwrap();
    let tracked = TRACKED_WINDOWS.get_or_init(|| Mutex::new(HashMap::new()));
    // This validates PID and ownership markers, including HWND reuse within a PID.
    controller.prune_managed_windows();
    if snapshot.applications.is_empty() {
        tracked.lock().unwrap().clear();
        controller.configure_drag_rules(Vec::new());
        update_drag_hook(false);
        statuses.write().unwrap().clear();
        return false;
    }
    let windows = enumerate_windows();
    let matches = crate::rules::matching_windows(&snapshot.applications, &windows);
    let next_tracked: HashMap<_, _> = matches
        .iter()
        .flatten()
        .map(|window| (window.hwnd, window.process_id))
        .collect();
    *tracked.lock().unwrap() = next_tracked;
    let mut drag_rules = HashMap::new();
    for (rule, windows) in snapshot.applications.iter().zip(&matches) {
        if rule.enabled {
            for window in windows {
                drag_rules.entry(window.hwnd).or_insert(rule.drag_mode);
            }
        }
    }
    controller.configure_drag_rules(drag_rules.into_iter().collect());
    let mut next = HashMap::new();
    for (rule, matches) in snapshot.applications.iter().zip(matches) {
        let status = if matches.is_empty() {
            RuleStatus::NotRunning
        } else if !snapshot.automatic_application || !rule.enabled {
            if matches.iter().all(|window| window.is_borderless) {
                RuleStatus::Borderless
            } else {
                RuleStatus::Running
            }
        } else {
            let mut failure = None;
            for window in &matches {
                if let Err(error) = controller.make_borderless(window.hwnd) {
                    failure = Some(error.to_string());
                }
            }
            failure.map(RuleStatus::Failed).unwrap_or_else(|| {
                if matches.iter().all(|window| is_borderless(window.hwnd)) {
                    RuleStatus::Borderless
                } else {
                    RuleStatus::Running
                }
            })
        };
        next.insert(rule.id, status);
    }
    let needs_elevation = next.values().any(|status| matches!(status, RuleStatus::Failed(message) if message.contains("elevation_required")));
    *statuses.write().unwrap() = next;
    update_drag_hook(snapshot.automatic_application && controller.has_strip_targets());
    needs_elevation
}

fn notify_elevation(previous: &mut bool, current: bool, sender: Option<&mpsc::SyncSender<bool>>) {
    if *previous != current {
        if let Some(sender) = sender {
            // Do not block the window scanner on a permission prompt.
            if sender.try_send(current).is_err() {
                return;
            }
        }
        *previous = current;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_bursts_queue_one_scan_until_consumed_and_retry_after_disconnect() {
        let pending = AtomicBool::new(false);
        let (sender, receiver) = mpsc::channel();
        let mut wakeups = 0;
        for _ in 0..10000 {
            wakeups += enqueue_scan(&pending, &sender) as usize;
        }
        assert_eq!(wakeups, 1);
        assert_eq!(receiver.try_recv(), Ok(()));
        assert_eq!(receiver.try_recv(), Err(mpsc::TryRecvError::Empty));
        pending.store(false, Ordering::Release);
        assert!(enqueue_scan(&pending, &sender));
        assert_eq!(receiver.try_recv(), Ok(()));
        pending.store(false, Ordering::Release);
        drop(receiver);
        assert!(!enqueue_scan(&pending, &sender));
        assert!(!pending.load(Ordering::Acquire));
        let (new_sender, new_receiver) = mpsc::channel();
        assert!(enqueue_scan(&pending, &new_sender));
        assert_eq!(new_receiver.try_recv(), Ok(()));
    }
    #[test]
    fn elevation_notification_is_independent_of_ui_and_only_sent_on_change() {
        let (sender, receiver) = mpsc::sync_channel(1);
        let mut required = false;
        notify_elevation(&mut required, true, Some(&sender));
        assert_eq!(receiver.try_recv(), Ok(true));
        for _ in 0..100 {
            notify_elevation(&mut required, true, Some(&sender));
        }
        assert_eq!(receiver.try_recv(), Err(mpsc::TryRecvError::Empty));
        notify_elevation(&mut required, false, Some(&sender));
        assert_eq!(receiver.try_recv(), Ok(false));
        notify_elevation(&mut required, true, Some(&sender));
        assert_eq!(receiver.try_recv(), Ok(true));
    }
    use std::time::Duration;
    #[test]
    fn restart_replaces_callback_controller_and_sender() {
        for _ in 0..2 {
            let controller = WindowController::default();
            let watcher = Watcher::start(
                Arc::new(RwLock::new(Config::default())),
                Arc::new(RwLock::new(HashMap::new())),
                controller.clone(),
            );
            assert!(
                MOUSE_CONTROLLER
                    .get()
                    .unwrap()
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .begin_shutdown()
            );
            assert!(
                !controller.begin_shutdown(),
                "callback must use the current controller"
            );
            drop(watcher);
            assert!(MOUSE_CONTROLLER.get().unwrap().lock().unwrap().is_none());
            assert!(MOUSE_DRAG_REQUESTS.get().unwrap().lock().unwrap().is_none());
        }
    }
    #[test]
    fn mouse_burst_keeps_one_wakeup_and_only_latest_coordinates() {
        let mailbox = DragMailbox::new();
        let mut wakeups = 0;
        for index in 0..10000 {
            wakeups += mailbox.publish(POINT {
                x: -index,
                y: index,
            }) as usize;
        }
        assert_eq!(wakeups, 1);
        let point = mailbox.consume();
        assert_eq!((point.x, point.y), (-9999, 9999));
        assert!(mailbox.publish(POINT { x: -32000, y: -200 }));
        let point = mailbox.consume();
        assert_eq!((point.x, point.y), (-32000, -200));
    }
    #[test]
    fn message_based_watcher_responds_to_notify_and_stops_without_polling() {
        let config = Arc::new(RwLock::new(Config::default()));
        let statuses = Arc::new(RwLock::new(HashMap::new()));
        statuses
            .write()
            .unwrap()
            .insert(Uuid::new_v4(), RuleStatus::Running);
        let watcher = Watcher::start(config, statuses.clone(), WindowController::default());
        let wait_for_scan = || {
            let deadline = std::time::Instant::now() + Duration::from_secs(1);
            while !statuses.read().unwrap().is_empty() && std::time::Instant::now() < deadline {
                thread::sleep(Duration::from_millis(5));
            }
            assert!(statuses.read().unwrap().is_empty());
        };
        wait_for_scan();
        statuses
            .write()
            .unwrap()
            .insert(Uuid::new_v4(), RuleStatus::Running);
        watcher.notify();
        wait_for_scan();
        let before = std::time::Instant::now();
        drop(watcher);
        assert!(before.elapsed() < Duration::from_secs(1));
    }
}
