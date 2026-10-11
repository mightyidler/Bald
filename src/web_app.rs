use std::{
    collections::HashMap,
    io::Cursor,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use base64::{Engine, engine::general_purpose::STANDARD};
use image::{DynamicImage, ImageFormat, RgbaImage};
use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, Emitter, Manager, State, WindowEvent,
    menu::{Menu, MenuItem},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_updater::UpdaterExt;
use uuid::Uuid;

use crate::{
    border_service::BorderService,
    config::Config,
    i18n::Language,
    rules::{ApplicationRule, DragMode},
    single_instance::SingleInstance,
    startup,
    watcher::RuleStatus,
    window_manager::{enumerate_windows, executable_icon},
};

struct AppState {
    update_running: Arc<AtomicBool>,
    shutting_down: Arc<AtomicBool>,
    elevation_attempted: Arc<AtomicBool>,
    elevation_error: Arc<Mutex<Option<String>>>,
    changes: Arc<Mutex<()>>,
    revision: AtomicU64,
    config: Arc<RwLock<Config>>,
    service: Arc<Mutex<BorderService>>,
    icon_cache: crate::icon_cache::IconCache,
    startup_error: Mutex<Option<String>>,
    ui_snapshot: Mutex<Option<crate::ui_window_state::UiWindowState>>,
    ui_closing: AtomicBool,
    ui_reopen_pending: AtomicBool,
}

struct UpdateGuard(Arc<AtomicBool>);
impl UpdateGuard {
    fn acquire(running: Arc<AtomicBool>) -> Option<Self> {
        running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Self(running))
    }
}
impl Drop for UpdateGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl AppState {
    fn restore_before_exit(&self) -> Result<(), String> {
        let _change = self.changes.lock().unwrap();
        let mut service = self.service.lock().unwrap();
        if self.shutting_down.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        if let Err(error) = service.shutdown() {
            self.shutting_down.store(false, Ordering::Release);
            return Err(error.to_string());
        }
        Ok(())
    }
}

impl Drop for AppState {
    fn drop(&mut self) {
        if let Err(error) = self.restore_before_exit() {
            eprintln!("exit restoration failed: {error}");
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UiState {
    revision: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    config: Option<Config>,
    statuses: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    application_icons: Option<HashMap<String, String>>,
    startup_enabled: bool,
    ui_visible: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    startup_error: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WindowChoice {
    hwnd: isize,
    process_id: u32,
    title: String,
    executable_name: String,
    executable_path: Option<String>,
    class_name: String,
    icon: Option<String>,
}

fn status_name(status: &RuleStatus) -> &'static str {
    match status {
        RuleStatus::NotRunning => "not_running",
        RuleStatus::Running => "running",
        RuleStatus::Borderless => "borderless",
        RuleStatus::Failed(message) if message.contains("elevation_required") => {
            "requires_elevation"
        }
        RuleStatus::Failed(_) => "failed",
    }
}

fn icon_data_url(path: Option<&str>) -> Option<String> {
    let icon = executable_icon(path?)?;
    let image = RgbaImage::from_raw(icon.width, icon.height, icon.rgba)?;
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, ImageFormat::Png)
        .ok()?;
    Some(format!(
        "data:image/png;base64,{}",
        STANDARD.encode(bytes.into_inner())
    ))
}

fn cached_icon_data_url(state: &AppState, path: Option<&str>) -> Option<String> {
    let path = path?;
    state
        .icon_cache
        .get_or_extract(path, || icon_data_url(Some(path)))
}

#[tauri::command]
async fn get_state(app: AppHandle, known_revision: Option<u64>) -> Result<UiState, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let mut snapshot = read_ui_state(&state, known_revision)?;
        snapshot.ui_visible = ui_visible(&app);
        Ok(snapshot)
    })
    .await
    .map_err(|error| error.to_string())?
}

fn read_ui_state(state: &AppState, known_revision: Option<u64>) -> Result<UiState, String> {
    let _change = state.changes.lock().unwrap();
    let revision = state.revision.load(Ordering::Acquire);
    let config = (known_revision != Some(revision)).then(|| state.config.read().unwrap().clone());
    let statuses = state
        .service
        .lock()
        .unwrap()
        .statuses()
        .map_err(|error| error.to_string())?
        .iter()
        .map(|(id, value)| (id.to_string(), status_name(value).to_owned()))
        .collect();
    let application_icons = config.as_ref().map(|config| {
        config
            .applications
            .iter()
            .filter_map(|rule| {
                cached_icon_data_url(state, rule.executable_path.as_deref())
                    .map(|icon| (rule.id.to_string(), icon))
            })
            .collect()
    });
    Ok(UiState {
        revision,
        config,
        statuses,
        application_icons,
        startup_enabled: state.config.read().unwrap().startup_enabled,
        ui_visible: false,
        startup_error: state.startup_error.lock().unwrap().clone(),
    })
}

#[tauri::command]
async fn list_windows(app: AppHandle) -> Result<Vec<WindowChoice>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        enumerate_windows()
            .into_iter()
            .map(|window| WindowChoice {
                hwnd: window.hwnd,
                process_id: window.process_id,
                icon: cached_icon_data_url(&state, window.executable_path.as_deref()),
                title: window.title,
                executable_name: window.executable_name,
                executable_path: window.executable_path,
                class_name: window.class_name,
            })
            .collect()
    })
    .await
    .map_err(|error| error.to_string())
}

async fn change_config(
    app: AppHandle,
    change: impl FnOnce(&mut Config) -> Result<(), String> + Send + 'static,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let _change = state.changes.lock().unwrap();
        if state.shutting_down.load(Ordering::Acquire) {
            return Err("BALD is shutting down".into());
        }
        let previous = state.config.read().unwrap().clone();
        let mut candidate = previous.clone();
        change(&mut candidate)?;
        if candidate == previous {
            return Ok(());
        }
        save_and_apply(
            &previous,
            &candidate,
            |config| config.save().map_err(|error| error.to_string()),
            |config| apply_config(&state, config, &previous),
        )?;
        *state.config.write().unwrap() = candidate;
        state.revision.fetch_add(1, Ordering::Release);
        Ok(())
    })
    .await
    .map_err(|error| error.to_string())?
}

fn save_and_apply(
    previous: &Config,
    candidate: &Config,
    mut save: impl FnMut(&Config) -> Result<(), String>,
    mut apply: impl FnMut(&Config) -> Result<(), String>,
) -> Result<(), String> {
    save(candidate)?;
    if let Err(error) = apply(candidate) {
        let disk = save(previous);
        let service = apply(previous);
        return Err(format!(
            "{error}; rollback disk: {}; rollback apply: {}",
            disk.err().unwrap_or_else(|| "ok".into()),
            service.err().unwrap_or_else(|| "ok".into())
        ));
    }
    Ok(())
}

fn apply_config(state: &AppState, candidate: &Config, previous: &Config) -> Result<(), String> {
    if native_settings_changed(candidate, previous) || candidate == previous {
        let mut service = state.service.lock().unwrap();
        service
            .sync(candidate.clone())
            .map_err(|error| error.to_string())?;
        if !candidate.automatic_application && previous.automatic_application {
            service.restore(None).map_err(|error| error.to_string())?;
        } else {
            for removed in previous
                .applications
                .iter()
                .filter(|rule| !candidate.applications.iter().any(|next| next.id == rule.id))
            {
                service
                    .restore(Some(removed.clone()))
                    .map_err(|error| error.to_string())?;
            }
        }
        for rule in &candidate.applications {
            if previous
                .applications
                .iter()
                .any(|old| old.id == rule.id && old.drag_mode != rule.drag_mode)
            {
                service
                    .drag(rule.clone())
                    .map_err(|error| error.to_string())?;
            }
        }
    }
    if candidate.startup_enabled != previous.startup_enabled || candidate == previous {
        startup::set_enabled(candidate.startup_enabled).map_err(|error| error.to_string())?;
        *state.startup_error.lock().unwrap() = None;
    }
    Ok(())
}

fn native_settings_changed(candidate: &Config, previous: &Config) -> bool {
    candidate.automatic_application != previous.automatic_application
        || candidate.applications != previous.applications
}

#[tauri::command]
async fn add_application(choice: WindowChoice, app: AppHandle) -> Result<(), String> {
    change_config(app, move |config| {
        let window = selected_window(&choice)?;
        if !config
            .applications
            .iter()
            .any(|rule| rule.duplicates(&window))
        {
            config
                .applications
                .push(ApplicationRule::from_window(&window));
        }
        Ok(())
    })
    .await
}

fn selected_window(choice: &WindowChoice) -> Result<crate::window_manager::WindowInfo, String> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::IsWindow;
    let hwnd = HWND(choice.hwnd as *mut std::ffi::c_void);
    if !unsafe { IsWindow(Some(hwnd)).as_bool() } {
        return Err("window is no longer available".to_owned());
    }
    let window = crate::window_manager::describe_window(hwnd);
    if !selection_matches(choice, &window) || !unsafe { IsWindow(Some(hwnd)).as_bool() } {
        return Err("selected window identity changed".to_owned());
    }
    Ok(window)
}

fn selection_matches(choice: &WindowChoice, window: &crate::window_manager::WindowInfo) -> bool {
    window.hwnd == choice.hwnd
        && window.process_id == choice.process_id
        && window.executable_path == choice.executable_path
        && window.class_name == choice.class_name
        && window
            .executable_name
            .eq_ignore_ascii_case(&choice.executable_name)
}

#[tauri::command]
async fn remove_application(id: String, app: AppHandle) -> Result<(), String> {
    let id = Uuid::parse_str(&id).map_err(|error| error.to_string())?;
    change_config(app, move |config| {
        config.applications.retain(|rule| rule.id != id);
        Ok(())
    })
    .await
}

#[tauri::command]
async fn set_application_drag_mode(
    id: String,
    mode: DragMode,
    app: AppHandle,
) -> Result<(), String> {
    let id = Uuid::parse_str(&id).map_err(|error| error.to_string())?;
    change_config(app, move |config| {
        let rule = config
            .applications
            .iter_mut()
            .find(|rule| rule.id == id)
            .ok_or_else(|| "application no longer registered".to_owned())?;
        rule.drag_mode = mode;
        Ok(())
    })
    .await
}

#[tauri::command]
async fn set_automatic(enabled: bool, app: AppHandle) -> Result<(), String> {
    change_config(app, move |config| {
        config.automatic_application = enabled;
        Ok(())
    })
    .await
}

#[tauri::command]
async fn set_language(language: Language, app: AppHandle) -> Result<(), String> {
    change_config(app, move |config| {
        config.language = language;
        Ok(())
    })
    .await
}

#[tauri::command]
async fn set_theme(theme: String, app: AppHandle) -> Result<(), String> {
    if !matches!(theme.as_str(), "system" | "light" | "dark") {
        return Err("unsupported theme".to_owned());
    }
    change_config(app, move |config| {
        config.theme = theme;
        Ok(())
    })
    .await
}

#[tauri::command]
async fn set_startup(enabled: bool, app: AppHandle) -> Result<(), String> {
    change_config(app, move |config| {
        config.startup_enabled = enabled;
        Ok(())
    })
    .await
}

#[tauri::command]
async fn reset_settings(app: AppHandle) -> Result<(), String> {
    change_config(app, |config| {
        *config = Config::default();
        Ok(())
    })
    .await
}

#[tauri::command]
async fn exit_app(app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<AppState>().restore_before_exit()?;
        app.exit(0);
        Ok(())
    })
    .await
    .map_err(|error| error.to_string())?
}

fn request_exit(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        if let Err(error) = exit_app(app.clone()).await {
            open_main_window(&app);
            let _ = app.emit("bald-error", error);
        }
    });
}

#[tauri::command]
async fn elevate_border_service(
    state: State<'_, AppState>,
    retry: Option<bool>,
) -> Result<(), String> {
    let changes = state.changes.clone();
    let service = state.service.clone();
    let config = state.config.clone();
    let attempted = state.elevation_attempted.clone();
    let error = state.elevation_error.clone();
    let closing = state.shutting_down.clone();
    tauri::async_runtime::spawn_blocking(move || {
        elevate_if_needed(
            &service,
            &config,
            &changes,
            &attempted,
            &error,
            &closing,
            retry.unwrap_or(false),
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

fn elevate_if_needed(
    service: &Arc<Mutex<BorderService>>,
    config: &Arc<RwLock<Config>>,
    changes: &Arc<Mutex<()>>,
    attempted: &AtomicBool,
    last_error: &Mutex<Option<String>>,
    closing: &AtomicBool,
    retry: bool,
) -> Result<(), String> {
    let _change = changes.lock().unwrap();
    if closing.load(Ordering::Acquire) {
        return Ok(());
    }
    if !retry
        && attempted.load(Ordering::Acquire)
        && let Some(error) = last_error.lock().unwrap().clone()
    {
        return Err(error);
    }
    let mut service = service.lock().unwrap();
    if !service.statuses().map_err(|error| error.to_string())?.values()
        .any(|status| matches!(status, RuleStatus::Failed(message) if message.contains("elevation_required"))) {
        return Ok(());
    }
    attempted.store(true, Ordering::Release);
    *last_error.lock().unwrap() = None;
    let snapshot = config.read().unwrap().clone();
    let result = service.elevate(snapshot).map_err(|error| error.to_string());
    *last_error.lock().unwrap() = result.as_ref().err().cloned();
    result
}

fn ui_visible(app: &AppHandle) -> bool {
    app.get_webview_window("main")
        .and_then(|window| window.hwnd().ok())
        .is_some_and(|hwnd| unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{IsIconic, IsWindowVisible};
            let hwnd = windows::Win32::Foundation::HWND(hwnd.0);
            IsWindowVisible(hwnd).as_bool() && !IsIconic(hwnd).as_bool()
        })
}

fn emit_ui_visibility(app: &AppHandle) {
    let _ = app.emit("bald-ui-visible", ui_visible(app));
}

#[tauri::command]
fn get_ui_visibility(app: AppHandle) -> bool {
    ui_visible(&app)
}

fn capture_ui_state(
    app: &AppHandle,
) -> Result<Option<crate::ui_window_state::UiWindowState>, String> {
    if app.get_webview_window("main").is_none() {
        return Ok(*app.state::<AppState>().ui_snapshot.lock().unwrap());
    }
    app.get_webview_window("main")
        .map(|window| {
            let hwnd = window.hwnd().map_err(|error| error.to_string())?;
            crate::ui_window_state::UiWindowState::capture(windows::Win32::Foundation::HWND(hwnd.0))
                .map_err(|error| error.to_string())
        })
        .transpose()
}

fn restart_arguments(
    snapshot: Option<crate::ui_window_state::UiWindowState>,
) -> Result<Vec<String>, String> {
    let mut arguments = vec!["--replace".to_owned(), "--background".to_owned()];
    if let Some(snapshot) = snapshot {
        arguments.push(snapshot.argument().map_err(|error| error.to_string())?);
    }
    Ok(arguments)
}

async fn restart_preserving_ui(app: &AppHandle) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let arguments = restart_arguments(capture_ui_state(app)?)?;
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<AppState>().restore_before_exit()?;
        let spawned = std::process::Command::new(executable)
            .args(arguments)
            .creation_flags(0x0800_0000)
            .spawn();
        if let Err(error) = spawned {
            let state = app.state::<AppState>();
            state
                .service
                .lock()
                .unwrap()
                .resume()
                .map_err(|resume| format!("restart: {error}; resume: {resume}"))?;
            state.shutting_down.store(false, Ordering::Release);
            return Err(error.to_string());
        }
        app.exit(0);
        Ok(())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
fn minimize_window(window: tauri::WebviewWindow) -> Result<(), String> {
    sync_visible_window(&window)?;
    window.minimize().map_err(|error| error.to_string())?;
    let _ = window.emit("bald-ui-visible", false);
    Ok(())
}

#[tauri::command]
fn close_window(window: tauri::WebviewWindow) -> Result<(), String> {
    let app = window.app_handle();
    let state = app.state::<AppState>();
    if state.ui_closing.swap(true, Ordering::AcqRel) {
        return Ok(());
    }
    let result = (|| {
        let hwnd = window.hwnd().map_err(|error| error.to_string())?;
        let snapshot = crate::ui_window_state::UiWindowState::capture(
            windows::Win32::Foundation::HWND(hwnd.0),
        )
        .map_err(|error| error.to_string())?;
        window.hide().map_err(|error| error.to_string())?;
        *state.ui_snapshot.lock().unwrap() = Some(snapshot.hidden());
        emit_ui_visibility(app);
        window.destroy().map_err(|error| error.to_string())
    })();
    if result.is_err() {
        state.ui_closing.store(false, Ordering::Release);
    }
    result
}

fn sync_visible_window(window: &tauri::WebviewWindow) -> Result<(), String> {
    // Native restart restoration bypasses Tao's cached VISIBLE flag. Sync it
    // only for explicit user actions on an already-visible window.
    if window.is_visible().map_err(|error| error.to_string())? {
        window.show().map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn start_window_drag(window: tauri::WebviewWindow) {
    let _ = window.start_dragging();
}

#[tauri::command]
fn set_window_height(window: tauri::WebviewWindow, height: f64) -> f64 {
    let monitor = window.current_monitor().ok().flatten();
    let scale = monitor
        .as_ref()
        .map(|monitor| monitor.scale_factor())
        .unwrap_or_else(|| window.scale_factor().unwrap_or(1.0));
    let maximum = monitor
        .map(|monitor| monitor.work_area().size.height as f64 / scale - 32.0)
        .unwrap_or(720.0);
    let height = height.clamp(480.0, (maximum - 8.0).max(480.0));
    let physical_width = (488.0 * scale).round() as u32;
    let physical_height = ((height + 8.0) * scale).round() as u32;
    let _ = window.set_size(tauri::PhysicalSize::new(physical_width, physical_height));
    physical_height as f64 / scale - 8.0
}

#[tauri::command]
fn get_app_version(app: tauri::AppHandle) -> String {
    app.package_info().version.to_string()
}

#[tauri::command]
async fn check_for_updates_manual(app: tauri::AppHandle) -> Result<String, String> {
    let Some(_update) = UpdateGuard::acquire(app.state::<AppState>().update_running.clone()) else {
        return Ok("BUSY".into());
    };
    let updater = app.updater().map_err(|error| error.to_string())?;
    match updater.check().await.map_err(|error| error.to_string())? {
        Some(update) => {
            update
                .download_and_install(|_, _| {}, || {})
                .await
                .map_err(|error| error.to_string())?;
            restart_preserving_ui(&app).await?;
            Ok("INSTALLED".to_owned())
        }
        None => Ok("LATEST".to_owned()),
    }
}

async fn install_background_update(app: tauri::AppHandle) {
    let Some(_update) = UpdateGuard::acquire(app.state::<AppState>().update_running.clone()) else {
        return;
    };
    let Ok(updater) = app.updater() else {
        return;
    };
    let Ok(Some(update)) = updater.check().await else {
        return;
    };
    if update.download_and_install(|_, _| {}, || {}).await.is_ok()
        && let Err(error) = restart_preserving_ui(&app).await
    {
        eprintln!("update restart failed: {error}");
    }
}

fn create_main_window(app: &AppHandle) -> anyhow::Result<tauri::WebviewWindow> {
    if let Some(window) = app.get_webview_window("main") {
        return Ok(window);
    }
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == "main")
        .ok_or_else(|| anyhow::anyhow!("main window configuration is missing"))?;
    let window = tauri::WebviewWindowBuilder::from_config(app, config)?.build()?;
    let configured: anyhow::Result<()> = (|| {
        let native = window.hwnd()?;
        crate::own_window_frame::install(windows::Win32::Foundation::HWND(native.0))?;
        let scale = window.scale_factor().unwrap_or(1.0);
        window.set_size(tauri::PhysicalSize::new(
            (488.0 * scale).round() as u32,
            (720.0 * scale).round() as u32,
        ))?;
        if let Some(snapshot) = *app.state::<AppState>().ui_snapshot.lock().unwrap() {
            snapshot
                .hidden()
                .apply(windows::Win32::Foundation::HWND(native.0))?;
        }
        Ok(())
    })();
    if let Err(error) = configured {
        if let Err(cleanup) = window.destroy() {
            return Err(anyhow::anyhow!(
                "initialize settings: {error:#}; release incomplete window: {cleanup}"
            ));
        }
        return Err(error);
    }
    Ok(window)
}

fn permit_elevation_retry(attempted: &AtomicBool, last_error: &Mutex<Option<String>>) {
    // A fresh WebView cannot receive the old page's reopen event. An explicit
    // open permits one retry after a completed failure, never during a prompt.
    if last_error.lock().unwrap().take().is_some() {
        attempted.store(false, Ordering::Release);
    }
}

pub(crate) fn open_main_window(app: &AppHandle) {
    let queued = app.clone();
    if let Err(error) = app.run_on_main_thread(move || {
        let state = queued.state::<AppState>();
        if state.ui_closing.load(Ordering::Acquire) {
            state.ui_reopen_pending.store(true, Ordering::Release);
            return;
        }
        permit_elevation_retry(&state.elevation_attempted, &state.elevation_error);
        match create_main_window(&queued) {
            Ok(window) => {
                let replay_content =
                    !window.is_visible().unwrap_or(true) && !window.is_minimized().unwrap_or(true);
                if replay_content {
                    let _ = window.eval("window.dispatchEvent(new Event('bald-window-reopened'))");
                }
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
                emit_ui_visibility(&queued);
            }
            Err(error) => eprintln!("open settings failed: {error:#}"),
        }
    }) {
        eprintln!("queue settings window failed: {error}");
    }
}

pub fn run(instance: SingleInstance) -> anyhow::Result<()> {
    let _instance = instance;
    // The native activation handler holds only a Weak reference. Release this
    // owner after the event loop so AppState still restores frames on shutdown.
    let application = Arc::new(Mutex::new(None));
    let activation_owner = application.clone();
    let activation_window = Arc::new(Mutex::new(None));
    let activation_setup = activation_window.clone();
    let quiet = std::env::args().any(|arg| matches!(arg.as_str(), "--autostart" | "--background"));
    let restore_visible = std::env::args().any(|arg| arg == "--restore-visible");
    let restore_minimized = std::env::args().any(|arg| arg == "--restore-minimized");
    let restored_ui =
        std::env::args().find_map(|arg| crate::ui_window_state::UiWindowState::from_argument(&arg));
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |app| {
            let config = Arc::new(RwLock::new(Config::load()));
            let startup_error = startup::set_enabled(config.read().unwrap().startup_enabled)
                .err()
                .map(|error| format!("startup registration: {error:#}"));
            let (elevation_tx, elevation_rx) = std::sync::mpsc::sync_channel(1);
            let service = Arc::new(Mutex::new(BorderService::new(
                config.read().unwrap().clone(),
                Some(elevation_tx),
            )));
            let changes = Arc::new(Mutex::new(()));
            let shutting_down = Arc::new(AtomicBool::new(false));
            let elevation_attempted = Arc::new(AtomicBool::new(false));
            let elevation_error = Arc::new(Mutex::new(None));
            let weak_service = Arc::downgrade(&service);
            let weak_config = Arc::downgrade(&config);
            let weak_changes = Arc::downgrade(&changes);
            let closing = shutting_down.clone();
            let attempted = elevation_attempted.clone();
            let last_error = elevation_error.clone();
            std::thread::spawn(move || {
                loop {
                    let required =
                        match elevation_rx.recv_timeout(std::time::Duration::from_secs(4)) {
                            Ok(required) => required,
                            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                                let (Some(service), Some(changes)) =
                                    (weak_service.upgrade(), weak_changes.upgrade())
                                else {
                                    break;
                                };
                                if !closing.load(Ordering::Acquire)
                                    && let Ok(_change) = changes.try_lock()
                                    && let Ok(mut service) = service.try_lock()
                                    && let Err(error) = service.recover_disconnected_worker()
                                {
                                    eprintln!("border service recovery failed: {error}");
                                }
                                continue;
                            }
                        };
                    if !required {
                        attempted.store(false, Ordering::Release);
                        *last_error.lock().unwrap() = None;
                        continue;
                    }
                    let (Some(service), Some(config), Some(changes)) = (
                        weak_service.upgrade(),
                        weak_config.upgrade(),
                        weak_changes.upgrade(),
                    ) else {
                        break;
                    };
                    if let Err(error) = elevate_if_needed(
                        &service,
                        &config,
                        &changes,
                        &attempted,
                        &last_error,
                        &closing,
                        false,
                    ) {
                        eprintln!("automatic elevation failed: {error}");
                    }
                }
            });
            app.manage(AppState {
                update_running: Arc::new(AtomicBool::new(false)),
                shutting_down,
                elevation_attempted,
                elevation_error,
                changes,
                revision: AtomicU64::new(1),
                config,
                service,
                icon_cache: crate::icon_cache::IconCache::default(),
                startup_error: Mutex::new(startup_error),
                ui_snapshot: Mutex::new(restored_ui),
                ui_closing: AtomicBool::new(false),
                ui_reopen_pending: AtomicBool::new(false),
            });
            *activation_owner.lock().unwrap() = Some(app.handle().clone());
            *activation_setup.lock().unwrap() =
                Some(crate::ui_activation::create(&activation_owner)?);

            let updater_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                install_background_update(updater_app).await;
            });

            let show = MenuItem::with_id(app, "show", "열기", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "종료", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Bald")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => {
                        request_exit(app.clone());
                    }
                    "show" => open_main_window(app),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::DoubleClick {
                        button: MouseButton::Left,
                        ..
                    } = event
                    {
                        open_main_window(tray.app_handle());
                    }
                })
                .build(app)?;
            if let Some(snapshot) = restored_ui {
                if snapshot.visible() {
                    let window = create_main_window(app.handle())?;
                    let hwnd = window.hwnd()?;
                    snapshot.apply(windows::Win32::Foundation::HWND(hwnd.0))?;
                }
            } else if restore_visible || restore_minimized {
                {
                    let window = create_main_window(app.handle())?;
                    use windows::Win32::{
                        Foundation::HWND,
                        UI::WindowsAndMessaging::{
                            SW_SHOWMINNOACTIVE, SW_SHOWNOACTIVATE, ShowWindow,
                        },
                    };
                    if let Ok(native) = window.hwnd() {
                        unsafe {
                            let _ = ShowWindow(
                                HWND(native.0),
                                if restore_minimized {
                                    SW_SHOWMINNOACTIVE
                                } else {
                                    SW_SHOWNOACTIVATE
                                },
                            );
                        }
                    }
                }
            } else if !quiet {
                let window = create_main_window(app.handle())?;
                let _ = window.show();
                let _ = window.set_focus();
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            if matches!(event, WindowEvent::Destroyed) {
                let app = window.app_handle();
                let state = app.state::<AppState>();
                state.ui_closing.store(false, Ordering::Release);
                if state.ui_reopen_pending.swap(false, Ordering::AcqRel) {
                    open_main_window(app);
                }
                return;
            }
            if matches!(event, WindowEvent::Focused(true) | WindowEvent::Resized(_)) {
                emit_ui_visibility(window.app_handle());
            }
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if let Some(main) = window.app_handle().get_webview_window("main") {
                    let _ = close_window(main);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            get_ui_visibility,
            list_windows,
            add_application,
            remove_application,
            set_application_drag_mode,
            set_automatic,
            set_language,
            set_theme,
            set_startup,
            reset_settings,
            exit_app,
            elevate_border_service,
            minimize_window,
            close_window,
            start_window_drag,
            set_window_height,
            get_app_version,
            check_for_updates_manual
        ])
        .build(tauri::generate_context!())
        .map_err(|error| anyhow::anyhow!(error.to_string()))?
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { code, api, .. } = event
                && let Some(state) = app.try_state::<AppState>()
                && !state.shutting_down.load(Ordering::Acquire)
            {
                api.prevent_exit();
                if code.is_some() {
                    request_exit(app.clone());
                }
            }
        });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reopening_permits_one_elevation_retry_only_after_completed_failure() {
        let attempted = AtomicBool::new(true);
        let error = Mutex::new(None);
        permit_elevation_retry(&attempted, &error);
        assert!(
            attempted.load(Ordering::Acquire),
            "ongoing prompt must not reset"
        );
        *error.lock().unwrap() = Some("cancelled".into());
        permit_elevation_retry(&attempted, &error);
        assert!(!attempted.load(Ordering::Acquire));
        attempted.store(true, Ordering::Release);
        permit_elevation_retry(&attempted, &error);
        assert!(
            attempted.load(Ordering::Acquire),
            "reopening cannot duplicate an ongoing retry"
        );
    }
    #[test]
    fn display_preferences_do_not_require_window_or_service_synchronization() {
        let previous = Config::default();
        let candidate = Config {
            theme: "dark".into(),
            language: Language::English,
            startup_enabled: false,
            ..previous.clone()
        };
        assert!(!native_settings_changed(&candidate, &previous));
        assert!(native_settings_changed(
            &Config {
                automatic_application: false,
                ..candidate
            },
            &previous
        ));
    }

    #[test]
    fn unchanged_state_serializes_without_settings_or_icons() {
        let json = serde_json::to_value(UiState {
            revision: 1,
            config: None,
            statuses: HashMap::new(),
            application_icons: None,
            startup_enabled: true,
            ui_visible: true,
            startup_error: None,
        })
        .unwrap();
        assert!(json.get("config").is_none());
        assert!(json.get("applicationIcons").is_none());
        assert_eq!(json["revision"], 1);
    }
    #[test]
    fn updates_share_one_slot_and_failure_or_cancellation_releases_it() {
        let running = Arc::new(AtomicBool::new(false));
        let background = UpdateGuard::acquire(running.clone()).unwrap();
        assert!(UpdateGuard::acquire(running.clone()).is_none());
        drop(background);
        let failed = std::panic::catch_unwind(|| {
            let _manual = UpdateGuard::acquire(running.clone()).unwrap();
            panic!("simulated cancelled download");
        });
        assert!(failed.is_err());
        assert!(UpdateGuard::acquire(running).is_some());
    }
    #[test]
    fn failed_save_does_not_apply_candidate() {
        let previous = Config::default();
        let candidate = Config {
            theme: "dark".into(),
            ..previous.clone()
        };
        let mut applied = 0;
        assert!(
            save_and_apply(
                &previous,
                &candidate,
                |_| Err("write denied".into()),
                |_| {
                    applied += 1;
                    Ok(())
                }
            )
            .is_err()
        );
        assert_eq!(applied, 0);
    }

    #[test]
    fn failed_apply_restores_persisted_and_service_settings() {
        let previous = Config::default();
        let candidate = Config {
            automatic_application: false,
            ..previous.clone()
        };
        let mut disk = previous.clone();
        let mut service = previous.clone();
        let result = save_and_apply(
            &previous,
            &candidate,
            |config| {
                disk = config.clone();
                Ok(())
            },
            |config| {
                service = config.clone();
                if config == &candidate {
                    Err("restore failed".into())
                } else {
                    Ok(())
                }
            },
        );
        assert!(result.is_err());
        assert_eq!(disk, previous);
        assert_eq!(service, previous);
    }

    #[test]
    fn rollback_failures_are_reported_instead_of_claiming_success() {
        let previous = Config::default();
        let candidate = Config {
            theme: "dark".into(),
            ..previous.clone()
        };
        let result = save_and_apply(
            &previous,
            &candidate,
            |config| {
                if config == &candidate {
                    Ok(())
                } else {
                    Err("rollback write denied".into())
                }
            },
            |_| Err("disconnected".into()),
        )
        .unwrap_err();
        assert!(result.contains("rollback write denied"));
        assert!(result.contains("rollback apply: disconnected"));
    }
    #[test]
    fn selection_uses_live_identity_and_accepts_title_changes() {
        let choice = WindowChoice {
            hwnd: 10,
            process_id: 20,
            title: "Old title".into(),
            executable_name: "Game.exe".into(),
            executable_path: Some(r"C:\Games\Game.exe".into()),
            class_name: "GameClass".into(),
            icon: None,
        };
        let mut live = crate::window_manager::WindowInfo {
            hwnd: 10,
            process_id: 20,
            title: "New title".into(),
            executable_name: choice.executable_name.clone(),
            executable_path: choice.executable_path.clone(),
            class_name: choice.class_name.clone(),
            is_borderless: false,
        };
        assert!(selection_matches(&choice, &live));
        live.hwnd = 11;
        assert!(!selection_matches(&choice, &live));
        live.hwnd = 10;
        live.process_id = 21;
        assert!(!selection_matches(&choice, &live));
        live.process_id = 20;
        live.executable_path = Some(r"C:\Other\Game.exe".into());
        assert!(!selection_matches(&choice, &live));
        live.executable_path = choice.executable_path.clone();
        live.class_name = "SplashClass".into();
        assert!(!selection_matches(&choice, &live));
    }

    #[test]
    fn selection_rejects_invalid_native_handle() {
        assert!(
            selected_window(&WindowChoice {
                hwnd: 0,
                process_id: 0,
                title: String::new(),
                executable_name: String::new(),
                executable_path: None,
                class_name: String::new(),
                icon: None,
            })
            .is_err()
        );
    }
}
