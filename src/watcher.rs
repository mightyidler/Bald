use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock, RwLock, mpsc},
    thread,
    time::{Duration, Instant},
};

use uuid::Uuid;
use windows::Win32::{
    Foundation::HWND,
    UI::{
        Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent},
        WindowsAndMessaging::{
            DispatchMessageW, EVENT_OBJECT_CREATE, EVENT_OBJECT_SHOW, MSG, OBJID_WINDOW, PM_REMOVE,
            PeekMessageW, TranslateMessage, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
        },
    },
};

use crate::{
    config::Config,
    window_manager::{WindowController, enumerate_windows},
};

#[derive(Debug, Clone, PartialEq, Eq)]
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
}

static WINDOW_EVENT_WAKE: OnceLock<Mutex<Option<mpsc::Sender<()>>>> = OnceLock::new();

unsafe extern "system" fn window_event_callback(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    object_id: i32,
    _child_id: i32,
    _thread_id: u32,
    _event_time: u32,
) {
    if hwnd.0.is_null() || object_id != OBJID_WINDOW.0 {
        return;
    }
    if let Some(sender) = WINDOW_EVENT_WAKE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap()
        .as_ref()
    {
        let _ = sender.send(());
    }
}

impl Watcher {
    pub fn start(
        config: Arc<RwLock<Config>>,
        statuses: Arc<RwLock<HashMap<Uuid, RuleStatus>>>,
        controller: WindowController,
    ) -> Self {
        let (wake_tx, wake_rx) = mpsc::channel();
        let (stop_tx, stop_rx) = mpsc::channel();
        *WINDOW_EVENT_WAKE
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap() = Some(wake_tx.clone());
        let thread = thread::spawn(move || {
            let hook = unsafe {
                SetWinEventHook(
                    EVENT_OBJECT_CREATE,
                    EVENT_OBJECT_SHOW,
                    None,
                    Some(window_event_callback),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
                )
            };
            let mut next_scan = Instant::now();
            loop {
                if stop_rx.try_recv().is_ok() {
                    break;
                }
                if Instant::now() >= next_scan {
                    scan(&config, &statuses, &controller);
                    next_scan = Instant::now() + Duration::from_secs(4);
                }
                if controller.poll_window_drag() {
                    next_scan = Instant::now();
                }
                let mut message = MSG::default();
                unsafe {
                    while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                        let _ = TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                }
                match wake_rx.recv_timeout(Duration::from_millis(16)) {
                    Ok(()) => next_scan = Instant::now(),
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
            if !hook.0.is_null() {
                unsafe {
                    let _ = UnhookWinEvent(hook);
                }
            }
        });
        Self {
            wake: wake_tx,
            stop: stop_tx,
            thread: Some(thread),
        }
    }

    pub fn notify(&self) {
        let _ = self.wake.send(());
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        let _ = self.wake.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn scan(
    config: &Arc<RwLock<Config>>,
    statuses: &Arc<RwLock<HashMap<Uuid, RuleStatus>>>,
    controller: &WindowController,
) {
    // Hold the read guard for the complete scan. Configuration mutations then wait
    // for an in-flight apply to finish before disabling a rule and restoring frames.
    let snapshot = config.read().unwrap();
    let windows = enumerate_windows();
    let mut next = HashMap::new();
    for rule in &snapshot.applications {
        let matches: Vec<_> = windows
            .iter()
            .filter(|window| rule.matches(window))
            .collect();
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
            for window in matches {
                if let Err(error) = controller.make_borderless(window.hwnd) {
                    failure = Some(error.to_string());
                }
            }
            failure
                .map(RuleStatus::Failed)
                .unwrap_or(RuleStatus::Borderless)
        };
        next.insert(rule.id, status);
    }
    *statuses.write().unwrap() = next;
}
