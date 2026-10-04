use std::{
    collections::HashMap,
    sync::{Arc, RwLock, mpsc},
    thread,
    time::{Duration, Instant},
};

use uuid::Uuid;

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

impl Watcher {
    pub fn start(
        config: Arc<RwLock<Config>>,
        statuses: Arc<RwLock<HashMap<Uuid, RuleStatus>>>,
        controller: WindowController,
    ) -> Self {
        let (wake_tx, wake_rx) = mpsc::channel();
        let (stop_tx, stop_rx) = mpsc::channel();
        let thread = thread::spawn(move || {
            let mut next_scan = Instant::now();
            loop {
                if stop_rx.try_recv().is_ok() {
                    break;
                }
                if Instant::now() >= next_scan {
                    scan(&config, &statuses, &controller);
                    next_scan = Instant::now() + Duration::from_secs(4);
                }
                controller.poll_window_drag();
                match wake_rx.recv_timeout(Duration::from_millis(16)) {
                    Ok(()) => next_scan = Instant::now(),
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
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
    let snapshot = config.read().unwrap().clone();
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
