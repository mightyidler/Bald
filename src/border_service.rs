//! Privileged window work without replacing or manipulating Bald's UI.
use std::{
    collections::HashMap,
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use uuid::Uuid;

use crate::{
    config::Config,
    rules::ApplicationRule,
    watcher::{RuleStatus, Watcher},
    window_manager::{WindowController, enumerate_windows},
};

const MAX_MESSAGE: usize = 1024 * 1024;

#[derive(Serialize, Deserialize)]
enum Request {
    Initialize(Config),
    Sync(Config),
    Status,
    Restore(Option<ApplicationRule>),
    Drag(ApplicationRule),
    Shutdown,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Response {
    statuses: HashMap<Uuid, RuleStatus>,
    error: Option<String>,
}

fn write_message<T: Serialize>(stream: &mut TcpStream, message: &T) -> Result<()> {
    let bytes = serde_json::to_vec(message)?;
    if bytes.len() > MAX_MESSAGE {
        bail!("border service message too large");
    }
    stream.write_all(&(bytes.len() as u32).to_le_bytes())?;
    stream.write_all(&bytes)?;
    Ok(())
}

fn read_message<T: DeserializeOwned>(stream: &mut TcpStream) -> Result<T> {
    let mut length = [0; 4];
    stream.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length > MAX_MESSAGE {
        bail!("border service message too large");
    }
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}

struct Local {
    config: Arc<RwLock<Config>>,
    statuses: Arc<RwLock<HashMap<Uuid, RuleStatus>>>,
    controller: WindowController,
    watcher: Option<Watcher>,
}

impl Local {
    fn new(config: Config) -> Self {
        let config = Arc::new(RwLock::new(config));
        let statuses = Arc::new(RwLock::new(HashMap::new()));
        let controller = WindowController::default();
        let watcher = Watcher::start(config.clone(), statuses.clone(), controller.clone());
        Self {
            config,
            statuses,
            controller,
            watcher: Some(watcher),
        }
    }

    fn restore(&self, rule: Option<&ApplicationRule>) -> Result<()> {
        let handles = self
            .controller
            .managed_windows()
            .iter()
            .filter(|window| rule.is_none_or(|rule| rule.matches(window)))
            .map(|window| window.hwnd)
            .collect();
        self.controller.restore_handles(handles)
    }

    fn shutdown(&mut self) -> Result<()> {
        // Stop scans before restoring; never leave two frame owners active.
        self.controller.begin_shutdown();
        drop(self.watcher.take());
        self.controller.restore_all_borders()
    }
}

struct Remote {
    stream: TcpStream,
}

fn prepare_worker_handshake(stream: &TcpStream) -> Result<()> {
    // Winsock accept inherits the listener's nonblocking mode. The framed
    // protocol uses read_exact/write_all, so it needs blocking connections.
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    Ok(())
}

impl Remote {
    fn request(&mut self, request: Request) -> Result<Response> {
        write_message(&mut self.stream, &request)?;
        let response: Response = read_message(&mut self.stream)?;
        if let Some(error) = &response.error {
            bail!("{error}");
        }
        Ok(response)
    }

    fn launch() -> Result<Self> {
        use windows::{
            Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_HIDE},
            core::PCWSTR,
        };
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let token = format!("{}{}", Uuid::new_v4(), Uuid::new_v4());
        let executable = std::env::current_exe()?;
        let executable: Vec<u16> = executable
            .as_os_str()
            .to_string_lossy()
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let arguments: Vec<u16> = format!(
            "--border-worker {} {token} {}",
            listener.local_addr()?,
            std::process::id()
        )
        .encode_utf16()
        .chain(Some(0))
        .collect();
        let operation: Vec<u16> = "runas\0".encode_utf16().collect();
        let result = unsafe {
            ShellExecuteW(
                None,
                PCWSTR(operation.as_ptr()),
                PCWSTR(executable.as_ptr()),
                PCWSTR(arguments.as_ptr()),
                None,
                SW_HIDE,
            )
        };
        if result.0 as isize <= 32 {
            bail!("elevation request failed: {}", result.0 as isize);
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    prepare_worker_handshake(&stream)?;
                    if read_message::<String>(&mut stream).ok().as_deref() != Some(&token) {
                        continue;
                    }
                    stream.set_read_timeout(Some(Duration::from_secs(60)))?;
                    stream.set_write_timeout(Some(Duration::from_secs(60)))?;
                    stream.set_nodelay(true)?;
                    return Ok(Self { stream });
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(error) => return Err(error.into()),
            }
        }
        bail!("elevated border service did not connect");
    }
}

pub struct BorderService {
    local: Option<Local>,
    remote: Option<Remote>,
}

impl BorderService {
    pub fn new(config: Config) -> Self {
        Self {
            local: Some(Local::new(config)),
            remote: None,
        }
    }

    pub fn sync(&mut self, config: Config) -> Result<()> {
        if let Some(remote) = &mut self.remote {
            remote.request(Request::Sync(config))?;
        } else if let Some(local) = &self.local {
            *local.config.write().unwrap() = config;
            if let Some(watcher) = &local.watcher {
                watcher.notify();
            }
        }
        Ok(())
    }

    pub fn statuses(&mut self) -> Result<HashMap<Uuid, RuleStatus>> {
        if let Some(remote) = &mut self.remote {
            Ok(remote.request(Request::Status)?.statuses)
        } else {
            Ok(self
                .local
                .as_ref()
                .context("border service stopped")?
                .statuses
                .read()
                .unwrap()
                .clone())
        }
    }

    pub fn restore(&mut self, rule: Option<ApplicationRule>) -> Result<()> {
        if let Some(remote) = &mut self.remote {
            remote.request(Request::Restore(rule))?;
            Ok(())
        } else {
            self.local
                .as_ref()
                .context("border service stopped")?
                .restore(rule.as_ref())
        }
    }

    pub fn drag(&mut self, rule: ApplicationRule) -> Result<()> {
        if let Some(remote) = &mut self.remote {
            remote.request(Request::Drag(rule))?;
        } else {
            let local = self.local.as_ref().context("border service stopped")?;
            for window in enumerate_windows().into_iter().filter(|window| {
                rule.matches(window)
                    && rule
                        .window_class_hint
                        .as_ref()
                        .is_none_or(|class| class.eq_ignore_ascii_case(&window.class_name))
            }) {
                local
                    .controller
                    .set_window_drag_mode(window.hwnd, rule.drag_mode);
                if rule.drag_mode == crate::rules::DragMode::Disabled {
                    local.controller.center_blocked_window(window.hwnd)?;
                }
            }
        }
        Ok(())
    }

    pub fn elevate(&mut self, config: Config) -> Result<()> {
        if self.remote.is_some() {
            return Ok(());
        }
        let remote = Remote::launch()?;
        // A cancelled UAC or launch failure leaves the existing owner intact.
        if let Some(local) = &mut self.local {
            let guard = local.config.write().unwrap();
            local.controller.restore_all_borders()?;
            local.controller.begin_shutdown();
            drop(guard);
            drop(local.watcher.take());
        }
        self.local = None;
        self.remote = Some(remote);
        self.remote
            .as_mut()
            .unwrap()
            .request(Request::Initialize(config))?;
        Ok(())
    }

    pub fn shutdown(&mut self) -> Result<()> {
        if let Some(mut remote) = self.remote.take() {
            remote.request(Request::Shutdown)?;
        }
        if let Some(local) = &mut self.local {
            local.shutdown()?;
        }
        Ok(())
    }
}

impl Drop for BorderService {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

pub fn run_worker(address: &str, token: &str) -> Result<()> {
    use windows::Win32::UI::HiDpi::{
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
    };
    // No Tauri event loop in this process to initialize DPI awareness for us.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let address: SocketAddr = address.parse()?;
    if !address.ip().is_loopback() || token.len() != 72 {
        bail!("invalid border service endpoint");
    }
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(5))?;
    stream.set_nodelay(true)?;
    stream.set_write_timeout(Some(Duration::from_secs(60)))?;
    write_message(&mut stream, &token)?;
    // Preserve the installed app's existing elevated startup path. This only
    // affects future launches, never replaces the UI connected to this worker.
    let _ = crate::startup::ensure_elevated_task();
    let mut service: Option<BorderService> = None;
    // A disconnected UI ends this loop; Drop restores all managed windows.
    while let Ok(request) = read_message::<Request>(&mut stream) {
        let shutdown = matches!(request, Request::Shutdown);
        let result: Result<HashMap<Uuid, RuleStatus>> = (|| {
            if let Request::Initialize(config) = request {
                if service.is_some() {
                    bail!("border service already initialized");
                }
                service = Some(BorderService::new(config));
                return Ok(HashMap::new());
            }
            let service = service.as_mut().context("border service not initialized")?;
            match request {
                Request::Sync(config) => service.sync(config)?,
                Request::Status => return service.statuses(),
                Request::Restore(rule) => service.restore(rule)?,
                Request::Drag(rule) => service.drag(rule)?,
                Request::Shutdown => service.shutdown()?,
                Request::Initialize(_) => unreachable!(),
            }
            Ok(HashMap::new())
        })();
        let response = match result {
            Ok(statuses) => Response {
                statuses,
                error: None,
            },
            Err(error) => Response {
                error: Some(error.to_string()),
                ..Default::default()
            },
        };
        write_message(&mut stream, &response)?;
        if shutdown {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pump_fixture_messages() {
        use windows::Win32::UI::WindowsAndMessaging::{
            DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage,
        };
        unsafe {
            let mut message = MSG::default();
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }

    fn pumped_request(remote: &mut Remote, request: Request) -> Response {
        // Production has Tauri's independent UI loop; this fixture must pump
        // its native owner while the worker sends frame notifications to it.
        std::thread::scope(|scope| {
            let request = scope.spawn(move || remote.request(request));
            while !request.is_finished() {
                pump_fixture_messages();
                std::thread::sleep(Duration::from_millis(5));
            }
            request.join().unwrap().unwrap()
        })
    }

    #[test]
    fn protocol_preserves_remote_status_and_errors() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let id = Uuid::new_v4();
        let thread = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            assert!(matches!(
                read_message::<Request>(&mut stream).unwrap(),
                Request::Status
            ));
            write_message(
                &mut stream,
                &Response {
                    statuses: HashMap::from([(id, RuleStatus::Borderless)]),
                    error: None,
                },
            )
            .unwrap();
            assert!(matches!(
                read_message::<Request>(&mut stream).unwrap(),
                Request::Shutdown
            ));
            write_message(
                &mut stream,
                &Response {
                    error: Some("restoration incomplete".to_owned()),
                    ..Default::default()
                },
            )
            .unwrap();
        });
        let mut remote = Remote {
            stream: TcpStream::connect(address).unwrap(),
        };
        assert_eq!(
            remote.request(Request::Status).unwrap().statuses[&id],
            RuleStatus::Borderless
        );
        assert!(
            remote
                .request(Request::Shutdown)
                .unwrap_err()
                .to_string()
                .contains("restoration incomplete")
        );
        thread.join().unwrap();
    }

    #[test]
    fn oversized_messages_are_rejected_before_allocation() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut sender = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut receiver, _) = listener.accept().unwrap();
        sender.write_all(&u32::MAX.to_le_bytes()).unwrap();
        assert!(read_message::<Request>(&mut receiver).is_err());
    }

    #[test]
    fn accepted_worker_waits_for_handshake_on_nonblocking_listener() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let sender = std::thread::spawn(move || {
            let mut stream = TcpStream::connect(address).unwrap();
            std::thread::sleep(Duration::from_millis(100));
            write_message(&mut stream, &"worker-token").unwrap();
        });
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("{error}"),
            }
        };
        prepare_worker_handshake(&stream).unwrap();
        assert_eq!(read_message::<String>(&mut stream).unwrap(), "worker-token");
        sender.join().unwrap();
    }

    #[test]
    #[ignore = "child entry point for the isolated border service process test"]
    fn worker_fixture() {
        let address = std::env::var("BALD_TEST_WORKER_ADDRESS").unwrap();
        let token = std::env::var("BALD_TEST_WORKER_TOKEN").unwrap();
        run_worker(&address, &token).unwrap();
    }

    #[test]
    #[ignore = "run separately: uses a fresh child watcher and native test windows, never games"]
    fn worker_process_keeps_ui_and_restores_on_disconnect() {
        use std::os::windows::process::CommandExt;
        use windows::{
            Win32::{
                Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
                System::LibraryLoader::GetModuleHandleW,
                UI::WindowsAndMessaging::{
                    CreateWindowExW, DefWindowProcW, DestroyWindow, GWL_STYLE, GetWindowLongPtrW,
                    RegisterClassW, SW_HIDE, SW_SHOWMINNOACTIVE, SW_SHOWNOACTIVATE, ShowWindow,
                    WNDCLASSW, WS_CAPTION, WS_OVERLAPPEDWINDOW,
                },
            },
            core::w,
        };
        unsafe extern "system" fn proc(
            hwnd: HWND,
            message: u32,
            wp: WPARAM,
            lp: LPARAM,
        ) -> LRESULT {
            unsafe { DefWindowProcW(hwnd, message, wp, lp) }
        }
        struct Fixtures(HWND, HWND);
        impl Drop for Fixtures {
            fn drop(&mut self) {
                unsafe {
                    let _ = DestroyWindow(self.0);
                    let _ = DestroyWindow(self.1);
                }
            }
        }
        unsafe {
            let module = HINSTANCE(GetModuleHandleW(None).unwrap().0);
            for class in [w!("Bald.WorkerFixture"), w!("Bald.WorkerUI")] {
                assert_ne!(
                    RegisterClassW(&WNDCLASSW {
                        lpfnWndProc: Some(proc),
                        hInstance: module,
                        lpszClassName: class,
                        ..Default::default()
                    }),
                    0
                );
            }
            let make = |class, title| {
                CreateWindowExW(
                    Default::default(),
                    class,
                    title,
                    WS_OVERLAPPEDWINDOW,
                    -24000,
                    -23000,
                    400,
                    300,
                    None,
                    None,
                    Some(module),
                    None,
                )
                .unwrap()
            };
            let fixtures = Fixtures(
                make(w!("Bald.WorkerUI"), w!("Bald fixture UI")),
                make(w!("Bald.WorkerFixture"), w!("Bald fixture game")),
            );
            let _ = ShowWindow(fixtures.0, SW_SHOWNOACTIVATE);
            let _ = ShowWindow(fixtures.1, SW_SHOWNOACTIVATE);
            let mut ui = crate::ui_window_state::UiWindowState::capture(fixtures.0).unwrap();
            let original_style = GetWindowLongPtrW(fixtures.1, GWL_STYLE);
            let info = crate::window_manager::WindowInfo {
                hwnd: fixtures.1.0 as isize,
                title: "Bald fixture game".to_owned(),
                executable_name: std::env::current_exe()
                    .unwrap()
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                executable_path: None,
                class_name: "Bald.WorkerFixture".to_owned(),
                is_borderless: false,
            };
            let mut rule = ApplicationRule::from_window(&info);
            // Match this fixture only, not the UI in this same test process.
            rule.executable_path = None;
            let id = rule.id;
            let mut config = Config {
                applications: vec![rule.clone()],
                ..Default::default()
            };
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let token = format!("{}{}", Uuid::new_v4(), Uuid::new_v4());
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "border_service::tests::worker_fixture",
                    "--ignored",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env(
                    "BALD_TEST_WORKER_ADDRESS",
                    listener.local_addr().unwrap().to_string(),
                )
                .env("BALD_TEST_WORKER_TOKEN", &token)
                .creation_flags(0x0800_0000)
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "worker did not connect");
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("{error}"),
                }
            };
            prepare_worker_handshake(&stream).unwrap();
            assert_eq!(read_message::<String>(&mut stream).unwrap(), token);
            stream
                .set_read_timeout(Some(Duration::from_secs(15)))
                .unwrap();
            let mut remote = Remote { stream };
            pumped_request(&mut remote, Request::Initialize(config.clone()));
            for cycle in 0..3 {
                if cycle == 1 {
                    let _ = ShowWindow(fixtures.0, SW_SHOWMINNOACTIVE);
                }
                if cycle == 2 {
                    let _ = ShowWindow(fixtures.0, SW_HIDE);
                }
                ui = crate::ui_window_state::UiWindowState::capture(fixtures.0).unwrap();
                config.automatic_application = true;
                pumped_request(&mut remote, Request::Sync(config.clone()));
                let deadline = Instant::now() + Duration::from_secs(5);
                loop {
                    if pumped_request(&mut remote, Request::Status)
                        .statuses
                        .get(&id)
                        == Some(&RuleStatus::Borderless)
                    {
                        break;
                    }
                    assert!(Instant::now() < deadline, "fixture was not made borderless");
                    std::thread::sleep(Duration::from_millis(50));
                }
                assert_eq!(
                    GetWindowLongPtrW(fixtures.1, GWL_STYLE) & WS_CAPTION.0 as isize,
                    0
                );
                assert_eq!(
                    crate::ui_window_state::UiWindowState::capture(fixtures.0).unwrap(),
                    ui
                );
                if cycle != 2 {
                    config.automatic_application = false;
                    pumped_request(&mut remote, Request::Sync(config.clone()));
                    pumped_request(&mut remote, Request::Restore(Some(rule.clone())));
                    assert_eq!(GetWindowLongPtrW(fixtures.1, GWL_STYLE), original_style);
                }
            }
            // Simulate the UI process closing its connection unexpectedly.
            drop(remote);
            let deadline = Instant::now() + Duration::from_secs(10);
            while child.try_wait().unwrap().is_none() {
                pump_fixture_messages();
                assert!(
                    Instant::now() < deadline,
                    "worker did not exit on disconnect"
                );
                std::thread::sleep(Duration::from_millis(50));
            }
            assert!(child.wait().unwrap().success());
            assert_eq!(GetWindowLongPtrW(fixtures.1, GWL_STYLE), original_style);
            assert_eq!(
                crate::ui_window_state::UiWindowState::capture(fixtures.0).unwrap(),
                ui
            );
            println!(
                "PASS: visible/minimized/hidden UI stays unchanged; OFF and disconnect restore target"
            );
        }
    }
}
