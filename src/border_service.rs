//! Privileged window work without replacing or manipulating Bald's UI.
use std::{
    collections::HashMap,
    io::{Read, Write},
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
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
    Activate,
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

fn write_message<T: Serialize>(stream: &mut impl Write, message: &T) -> Result<()> {
    let bytes = serde_json::to_vec(message)?;
    if bytes.len() > MAX_MESSAGE {
        bail!("border service message too large");
    }
    stream.write_all(&(bytes.len() as u32).to_le_bytes())?;
    stream.write_all(&bytes)?;
    Ok(())
}

fn read_message<T: DeserializeOwned>(stream: &mut impl Read) -> Result<T> {
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

#[cfg(test)]
mod pure_codec_tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn framed_responses_roundtrip_in_memory_including_service_errors() {
        let response = Response {
            error: Some("restore failed".into()),
            ..Response::default()
        };
        let mut bytes = Vec::new();
        write_message(&mut bytes, &response).unwrap();
        let decoded: Response = read_message(&mut Cursor::new(bytes)).unwrap();
        assert_eq!(decoded.error, response.error);
        assert!(decoded.statuses.is_empty());
    }

    #[test]
    fn truncated_or_invalid_frames_fail_without_socket_or_worker() {
        let mut bytes = Vec::new();
        write_message(&mut bytes, &Response::default()).unwrap();
        for end in 0..bytes.len() {
            assert!(read_message::<Response>(&mut Cursor::new(&bytes[..end])).is_err());
        }
        let mut invalid = Cursor::new([1, 0, 0, 0, b'{']);
        assert!(read_message::<Response>(&mut invalid).is_err());
    }

    #[test]
    fn oversized_frames_fail_before_reading_payload_or_writing_header() {
        let mut bytes = ((MAX_MESSAGE + 1) as u32).to_le_bytes().to_vec();
        bytes.extend_from_slice(b"unread payload");
        let mut input = Cursor::new(bytes);
        assert!(read_message::<Response>(&mut input).is_err());
        assert_eq!(input.position(), 4);
        let mut output = Vec::new();
        assert!(write_message(&mut output, &"x".repeat(MAX_MESSAGE + 1)).is_err());
        assert!(output.is_empty());
    }
}

struct Local {
    config: Arc<RwLock<Config>>,
    statuses: Arc<RwLock<HashMap<Uuid, RuleStatus>>>,
    controller: WindowController,
    watcher: Option<Watcher>,
    elevation: Option<std::sync::mpsc::SyncSender<bool>>,
}

impl Local {
    fn new(config: Config, elevation: Option<std::sync::mpsc::SyncSender<bool>>) -> Self {
        let mut local = Self::prepared(config);
        local.elevation = elevation;
        local.start();
        local
    }

    fn prepared(config: Config) -> Self {
        let config = Arc::new(RwLock::new(config));
        let statuses = Arc::new(RwLock::new(HashMap::new()));
        let controller = WindowController::default();
        Self {
            config,
            statuses,
            controller,
            watcher: None,
            elevation: None,
        }
    }

    fn start(&mut self) {
        if self.watcher.is_none() {
            self.controller.resume();
            self.watcher = Some(Watcher::start_with_elevation(
                self.config.clone(),
                self.statuses.clone(),
                self.controller.clone(),
                self.elevation.clone(),
            ));
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
    healthy: bool,
    process: Option<WorkerProcess>,
}

struct WorkerProcess(isize);
impl WorkerProcess {
    fn exited(&self) -> bool {
        use windows::Win32::{
            Foundation::{HANDLE, WAIT_OBJECT_0},
            System::Threading::WaitForSingleObject,
        };
        unsafe { WaitForSingleObject(HANDLE(self.0 as *mut _), 0) == WAIT_OBJECT_0 }
    }

    fn wait(&self) -> Result<()> {
        use windows::Win32::{
            Foundation::{HANDLE, WAIT_OBJECT_0},
            System::Threading::WaitForSingleObject,
        };
        if unsafe { WaitForSingleObject(HANDLE(self.0 as *mut _), 30_000) } != WAIT_OBJECT_0 {
            bail!("worker exit not confirmed; local window management remains paused");
        }
        Ok(())
    }
}
impl Drop for WorkerProcess {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(windows::Win32::Foundation::HANDLE(
                self.0 as *mut _,
            ));
        }
    }
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
    fn connected(stream: TcpStream) -> Self {
        Self {
            stream,
            healthy: true,
            process: None,
        }
    }

    fn request(&mut self, request: Request) -> Result<Response> {
        let timeout = if matches!(request, Request::Restore(_) | Request::Shutdown) {
            Duration::from_secs(60)
        } else {
            Duration::from_secs(5)
        };
        self.request_with_timeout(request, timeout)
    }

    fn request_with_timeout(&mut self, request: Request, timeout: Duration) -> Result<Response> {
        if !self.healthy {
            bail!("border service disconnected; retry elevation to recover");
        }
        let result = (|| {
            let deadline = Instant::now() + timeout;
            let bytes = serde_json::to_vec(&request)?;
            if bytes.len() > MAX_MESSAGE {
                bail!("border service message too large");
            }
            let mut frame = (bytes.len() as u32).to_le_bytes().to_vec();
            frame.extend_from_slice(&bytes);
            let mut remaining = frame.as_slice();
            while !remaining.is_empty() {
                self.stream
                    .set_write_timeout(Some(time_remaining(deadline)?))?;
                let written = self.stream.write(remaining)?;
                if written == 0 {
                    bail!("border service closed during write");
                }
                remaining = &remaining[written..];
            }
            let mut length = [0; 4];
            read_before(&mut self.stream, &mut length, deadline)?;
            let length = u32::from_le_bytes(length) as usize;
            if length > MAX_MESSAGE {
                bail!("border service message too large");
            }
            let mut bytes = vec![0; length];
            read_before(&mut self.stream, &mut bytes, deadline)?;
            Ok::<_, anyhow::Error>(serde_json::from_slice::<Response>(&bytes)?)
        })();
        let response = match result {
            Ok(response) => response,
            Err(error) => {
                self.healthy = false;
                let _ = self.stream.shutdown(Shutdown::Both);
                return Err(error.context("border service transport failed"));
            }
        };
        if let Some(error) = &response.error {
            bail!("{error}");
        }
        Ok(response)
    }

    fn retire(&mut self) -> Result<()> {
        self.healthy = false;
        let _ = self.stream.shutdown(Shutdown::Both);
        self.process
            .as_ref()
            .context("worker ownership cannot be confirmed")?
            .wait()
    }

    fn launch() -> Result<Self> {
        use windows::{
            Win32::UI::{
                Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW},
                WindowsAndMessaging::SW_HIDE,
            },
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
        let mut launch = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS,
            lpVerb: PCWSTR(operation.as_ptr()),
            lpFile: PCWSTR(executable.as_ptr()),
            lpParameters: PCWSTR(arguments.as_ptr()),
            nShow: SW_HIDE.0,
            ..Default::default()
        };
        unsafe {
            ShellExecuteExW(&mut launch).context("elevation request failed")?;
        }
        if launch.hProcess.0.is_null() {
            bail!("elevated worker process handle missing");
        }
        let process = WorkerProcess(launch.hProcess.0 as isize);
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
                    let mut remote = Self::connected(stream);
                    remote.process = Some(process);
                    return Ok(remote);
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
    pub fn new(config: Config, elevation: Option<std::sync::mpsc::SyncSender<bool>>) -> Self {
        Self {
            local: Some(Local::new(config, elevation)),
            remote: None,
        }
    }

    pub fn sync(&mut self, config: Config) -> Result<()> {
        self.recover_disconnected_worker()?;
        if let Some(remote) = &mut self.remote {
            remote.request(Request::Sync(config.clone()))?;
        }
        if let Some(local) = &self.local {
            *local.config.write().unwrap() = config;
            if self.remote.is_none()
                && let Some(watcher) = &local.watcher
            {
                watcher.notify();
            }
        }
        Ok(())
    }

    pub fn statuses(&mut self) -> Result<HashMap<Uuid, RuleStatus>> {
        self.recover_disconnected_worker()?;
        if let Some(remote) = &mut self.remote {
            match remote.request(Request::Status) {
                Ok(response) => Ok(response.statuses),
                Err(error) => {
                    self.recover_disconnected_worker()?;
                    Err(error)
                }
            }
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
            for window in enumerate_windows()
                .into_iter()
                .filter(|window| rule.matches_automatic_window(window))
            {
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
        if self.remote.as_ref().is_some_and(|remote| remote.healthy) {
            return Ok(());
        }
        self.recover_disconnected_worker()?;
        let remote = Remote::launch()?;
        self.handoff(remote, config)
    }

    pub(crate) fn recover_disconnected_worker(&mut self) -> Result<()> {
        if let Some(remote) = &mut self.remote
            && remote.process.as_ref().is_some_and(WorkerProcess::exited)
        {
            remote.healthy = false;
        }
        if let Some(remote) = &mut self.remote
            && !remote.healthy
        {
            remote.retire()?;
            self.remote = None;
            if let Some(local) = &mut self.local {
                local.start();
            }
        }
        Ok(())
    }

    fn handoff(&mut self, mut remote: Remote, config: Config) -> Result<()> {
        // Prepare the worker without starting a second frame owner.
        remote.request(Request::Initialize(config.clone()))?;
        // A cancelled UAC or launch failure leaves the existing owner intact.
        if let Some(local) = &mut self.local
            && let Err(error) = local.shutdown()
        {
            local.start();
            return Err(error);
        }
        self.remote = Some(remote);
        if let Err(error) = self.remote.as_mut().unwrap().request(Request::Activate) {
            self.remote.as_mut().unwrap().retire()?;
            self.remote = None;
            if let Some(local) = &mut self.local {
                local.start();
            }
            return Err(error);
        }
        Ok(())
    }

    pub fn shutdown(&mut self) -> Result<()> {
        if let Some(remote) = &mut self.remote {
            if remote.healthy {
                let result = remote.request(Request::Shutdown);
                // A framed restoration error retains the worker's snapshots
                // for retry. Transport failure instead requires confirmed exit.
                if remote.healthy {
                    result?;
                }
            }
            remote.retire()?;
        }
        self.remote = None;
        if let Some(local) = &mut self.local {
            local.shutdown()?;
        }
        Ok(())
    }

    pub(crate) fn resume(&mut self) -> Result<()> {
        if self.remote.is_some() {
            bail!("remote owner still active");
        }
        self.local
            .as_mut()
            .context("local service missing")?
            .start();
        Ok(())
    }
}

fn time_remaining(deadline: Instant) -> Result<Duration> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        bail!("border service request deadline exceeded");
    }
    Ok(remaining)
}

fn read_before(stream: &mut TcpStream, mut bytes: &mut [u8], deadline: Instant) -> Result<()> {
    while !bytes.is_empty() {
        stream.set_read_timeout(Some(time_remaining(deadline)?))?;
        let read = stream.read(bytes)?;
        if read == 0 {
            bail!("border service closed during response");
        }
        bytes = &mut bytes[read..];
    }
    Ok(())
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
    if let Err(error) = crate::startup::ensure_elevated_task() {
        eprintln!("elevated startup registration failed: {error}");
    }
    let mut service: Option<BorderService> = None;
    // A disconnected UI ends this loop; Drop restores all managed windows.
    while let Ok(request) = read_message::<Request>(&mut stream) {
        let shutdown = matches!(request, Request::Shutdown);
        let result: Result<HashMap<Uuid, RuleStatus>> = (|| {
            if let Request::Initialize(config) = request {
                if service.is_some() {
                    bail!("border service already initialized");
                }
                service = Some(BorderService {
                    local: Some(Local::prepared(config)),
                    remote: None,
                });
                return Ok(HashMap::new());
            }
            let service = service.as_mut().context("border service not initialized")?;
            match request {
                Request::Activate => service
                    .local
                    .as_mut()
                    .context("local service missing")?
                    .start(),
                Request::Sync(config) => service.sync(config)?,
                Request::Status => return service.statuses(),
                Request::Restore(rule) => service.restore(rule)?,
                Request::Drag(rule) => service.drag(rule)?,
                Request::Shutdown => service.shutdown()?,
                Request::Initialize(_) => unreachable!(),
            }
            Ok(HashMap::new())
        })();
        let shutdown = shutdown && result.is_ok();
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

    #[test]
    fn partial_response_disconnect_invalidates_stream_before_next_request() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut remote =
            Remote::connected(TcpStream::connect(listener.local_addr().unwrap()).unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            read_message::<Request>(&mut stream).unwrap();
            stream.write_all(&[20, 0]).unwrap();
        });
        assert!(remote.request(Request::Status).is_err());
        server.join().unwrap();
        assert!(!remote.healthy);
        let started = Instant::now();
        assert!(remote.request(Request::Status).is_err());
        assert!(started.elapsed() < Duration::from_millis(100));
    }

    #[test]
    fn response_deadline_includes_partial_reads_and_closes_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut remote =
            Remote::connected(TcpStream::connect(listener.local_addr().unwrap()).unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            read_message::<Request>(&mut stream).unwrap();
            let _ = stream.write_all(&[20, 0]);
            std::thread::sleep(Duration::from_millis(150));
        });
        assert!(
            remote
                .request_with_timeout(Request::Status, Duration::from_millis(40))
                .is_err()
        );
        assert!(!remote.healthy);
        server.join().unwrap();
    }

    #[test]
    fn initialization_failure_preserves_existing_owner_and_configuration() {
        let config = Config::default();
        let mut service = BorderService {
            local: Some(Local::prepared(config.clone())),
            remote: None,
        };
        let controller = service.local.as_ref().unwrap().controller.clone();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let remote = Remote::connected(TcpStream::connect(listener.local_addr().unwrap()).unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            assert!(matches!(
                read_message::<Request>(&mut stream).unwrap(),
                Request::Initialize(_)
            ));
            write_message(
                &mut stream,
                &Response {
                    error: Some("initialize rejected".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        });
        assert!(service.handoff(remote, config.clone()).is_err());
        server.join().unwrap();
        assert!(service.remote.is_none());
        assert_eq!(
            *service.local.as_ref().unwrap().config.read().unwrap(),
            config
        );
        assert!(
            controller.begin_shutdown(),
            "initialization failure must not shut down the previous owner"
        );
    }

    #[test]
    fn prepared_worker_has_no_watcher_or_frame_owner_before_activation() {
        let local = Local::prepared(Config::default());
        assert!(local.watcher.is_none());
        assert!(local.controller.managed_windows().is_empty());
    }

    #[test]
    fn disconnected_status_does_not_resume_without_worker_ownership() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut remote =
            Remote::connected(TcpStream::connect(listener.local_addr().unwrap()).unwrap());
        remote.healthy = false;
        let mut service = BorderService {
            local: Some(Local::prepared(Config::default())),
            remote: Some(remote),
        };
        assert!(
            service
                .statuses()
                .unwrap_err()
                .to_string()
                .contains("ownership")
        );
        assert!(service.remote.is_some());
        assert!(service.local.as_ref().unwrap().watcher.is_none());
    }

    #[test]
    fn status_disconnect_resumes_local_only_after_owned_process_exit() {
        use std::os::windows::{
            io::{AsHandle, IntoRawHandle},
            process::CommandExt,
        };
        let mut child = std::process::Command::new("cmd.exe")
            .args(["/D", "/Q"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(0x08000000)
            .spawn()
            .unwrap();
        let process = child.as_handle().try_clone_to_owned().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut remote =
            Remote::connected(TcpStream::connect(listener.local_addr().unwrap()).unwrap());
        remote.process = Some(WorkerProcess(process.into_raw_handle() as isize));
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            assert!(matches!(
                read_message::<Request>(&mut stream).unwrap(),
                Request::Status
            ));
            child
                .stdin
                .take()
                .unwrap()
                .write_all(b"exit /b 0\r\n")
                .unwrap();
            assert!(child.wait().unwrap().success());
        });
        let mut service = BorderService {
            local: Some(Local::prepared(Config {
                automatic_application: false,
                ..Config::default()
            })),
            remote: Some(remote),
        };
        assert!(
            service.statuses().is_err(),
            "the original transport failure remains visible"
        );
        server.join().unwrap();
        assert!(service.remote.is_none());
        assert!(service.local.as_ref().unwrap().watcher.is_some());
        assert!(service.statuses().is_ok());
    }

    #[test]
    fn exited_worker_is_recovered_without_ui_status_polling() {
        use std::os::windows::{
            io::{AsHandle, IntoRawHandle},
            process::CommandExt,
        };
        let mut child = std::process::Command::new("cmd.exe")
            .args(["/c", "exit", "0"])
            .creation_flags(0x08000000)
            .spawn()
            .unwrap();
        let process = child.as_handle().try_clone_to_owned().unwrap();
        assert!(child.wait().unwrap().success());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut remote =
            Remote::connected(TcpStream::connect(listener.local_addr().unwrap()).unwrap());
        remote.process = Some(WorkerProcess(process.into_raw_handle() as isize));
        let mut service = BorderService {
            local: Some(Local::prepared(Config {
                automatic_application: false,
                ..Config::default()
            })),
            remote: Some(remote),
        };
        service.recover_disconnected_worker().unwrap();
        assert!(service.remote.is_none());
        assert!(service.local.as_ref().unwrap().watcher.is_some());
        assert!(service.statuses().is_ok());
    }

    #[test]
    fn activation_failure_never_restarts_local_without_confirmed_worker_exit() {
        let mut service = BorderService {
            local: Some(Local::prepared(Config::default())),
            remote: None,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let remote = Remote::connected(TcpStream::connect(listener.local_addr().unwrap()).unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            assert!(matches!(
                read_message::<Request>(&mut stream).unwrap(),
                Request::Initialize(_)
            ));
            write_message(&mut stream, &Response::default()).unwrap();
            assert!(matches!(
                read_message::<Request>(&mut stream).unwrap(),
                Request::Activate
            ));
            write_message(
                &mut stream,
                &Response {
                    error: Some("activation failed".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        });
        assert!(
            service
                .handoff(remote, Config::default())
                .unwrap_err()
                .to_string()
                .contains("ownership cannot be confirmed")
        );
        server.join().unwrap();
        assert!(!service.remote.as_ref().unwrap().healthy);
        assert!(service.local.as_ref().unwrap().watcher.is_none());
        assert!(!service.local.as_ref().unwrap().controller.begin_shutdown());
    }

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
        let mut remote = Remote::connected(TcpStream::connect(address).unwrap());
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
                process_id: std::process::id(),
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
            let mut remote = Remote::connected(stream);
            pumped_request(&mut remote, Request::Initialize(config.clone()));
            pumped_request(&mut remote, Request::Activate);
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
