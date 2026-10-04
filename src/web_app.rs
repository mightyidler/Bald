use std::{
    collections::HashMap,
    io::Cursor,
    sync::{Arc, Mutex, RwLock},
};

use base64::{Engine, engine::general_purpose::STANDARD};
use image::{DynamicImage, ImageFormat, RgbaImage};
use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, Manager, State, WindowEvent,
    menu::{Menu, MenuItem},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_updater::UpdaterExt;
use uuid::Uuid;

use crate::{
    config::Config,
    i18n::Language,
    rules::ApplicationRule,
    single_instance::SingleInstance,
    startup,
    watcher::{RuleStatus, Watcher},
    window_manager::{WindowController, enumerate_windows, executable_icon},
};

struct AppState {
    config: Arc<RwLock<Config>>,
    statuses: Arc<RwLock<HashMap<Uuid, RuleStatus>>>,
    controller: WindowController,
    _watcher: Watcher,
    icon_cache: Mutex<HashMap<String, String>>,
}

impl AppState {
    fn restore_before_exit(&self) {
        if !self.controller.begin_shutdown() {
            return;
        }
        // A scan holds the matching read lock until it finishes applying styles.
        // Taking the write lock here waits for that work and prevents a late scan
        // from making a restored window borderless again during shutdown.
        let _config = self.config.write().unwrap();
        self.controller.restore_all_borders();
    }
}

impl Drop for AppState {
    fn drop(&mut self) {
        self.restore_before_exit();
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UiState {
    config: Config,
    statuses: HashMap<String, String>,
    application_icons: HashMap<String, String>,
    startup_enabled: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WindowChoice {
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
    let key = path.to_lowercase();
    if let Some(icon) = state.icon_cache.lock().unwrap().get(&key) {
        return Some(icon.clone());
    }
    let icon = icon_data_url(Some(path))?;
    state.icon_cache.lock().unwrap().insert(key, icon.clone());
    Some(icon)
}

#[tauri::command]
fn get_state(state: State<'_, AppState>) -> UiState {
    let config = state.config.read().unwrap().clone();
    let statuses = state
        .statuses
        .read()
        .unwrap()
        .iter()
        .map(|(id, value)| (id.to_string(), status_name(value).to_owned()))
        .collect();
    let application_icons = config
        .applications
        .iter()
        .filter_map(|rule| {
            cached_icon_data_url(&state, rule.executable_path.as_deref())
                .map(|icon| (rule.id.to_string(), icon))
        })
        .collect();
    UiState {
        config,
        statuses,
        application_icons,
        startup_enabled: state.config.read().unwrap().startup_enabled,
    }
}

#[tauri::command]
fn list_windows(state: State<'_, AppState>) -> Vec<WindowChoice> {
    enumerate_windows()
        .into_iter()
        .map(|window| WindowChoice {
            icon: cached_icon_data_url(&state, window.executable_path.as_deref()),
            title: window.title,
            executable_name: window.executable_name,
            executable_path: window.executable_path,
            class_name: window.class_name,
        })
        .collect()
}

fn persist(state: &AppState) -> Result<(), String> {
    state
        .config
        .read()
        .unwrap()
        .save()
        .map_err(|error| error.to_string())?;
    state._watcher.notify();
    Ok(())
}

fn restore_rule_windows(rule: &ApplicationRule, state: &AppState) {
    for window in enumerate_windows()
        .iter()
        .filter(|window| rule.matches(window))
    {
        let _ = state.controller.restore_borders(window.hwnd);
    }
}

#[tauri::command]
fn add_application(choice: WindowChoice, state: State<'_, AppState>) -> Result<(), String> {
    let window = enumerate_windows()
        .into_iter()
        .find(|window| {
            window.title == choice.title && window.executable_name == choice.executable_name
        })
        .ok_or_else(|| "window is no longer available".to_owned())?;
    let mut config = state.config.write().unwrap();
    if !config
        .applications
        .iter()
        .any(|rule| rule.duplicates(&window))
    {
        config
            .applications
            .push(ApplicationRule::from_window(&window));
    }
    drop(config);
    persist(&state)
}

#[tauri::command]
fn remove_application(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let id = Uuid::parse_str(&id).map_err(|error| error.to_string())?;
    let rule = state
        .config
        .read()
        .unwrap()
        .applications
        .iter()
        .find(|rule| rule.id == id)
        .cloned();
    state
        .config
        .write()
        .unwrap()
        .applications
        .retain(|rule| rule.id != id);
    persist(&state)?;
    if let Some(rule) = rule {
        restore_rule_windows(&rule, &state);
    }
    Ok(())
}

#[tauri::command]
fn set_automatic(enabled: bool, state: State<'_, AppState>) -> Result<(), String> {
    let rules = state.config.read().unwrap().applications.clone();
    state.config.write().unwrap().automatic_application = enabled;
    persist(&state)?;
    if !enabled {
        for rule in &rules {
            restore_rule_windows(rule, &state);
        }
    }
    Ok(())
}

#[tauri::command]
fn set_language(language: Language, state: State<'_, AppState>) -> Result<(), String> {
    state.config.write().unwrap().language = language;
    persist(&state)
}

#[tauri::command]
fn set_theme(theme: String, state: State<'_, AppState>) -> Result<(), String> {
    if !matches!(theme.as_str(), "system" | "light" | "dark") {
        return Err("unsupported theme".to_owned());
    }
    state.config.write().unwrap().theme = theme;
    persist(&state)
}

#[tauri::command]
fn set_startup(enabled: bool, state: State<'_, AppState>) -> Result<(), String> {
    startup::set_enabled(enabled).map_err(|error| error.to_string())?;
    state.config.write().unwrap().startup_enabled = enabled;
    persist(&state)
}

#[tauri::command]
fn reset_settings(state: State<'_, AppState>) -> Result<(), String> {
    let rules = state.config.read().unwrap().applications.clone();
    *state.config.write().unwrap() = Config::default();
    persist(&state)?;
    for rule in &rules {
        restore_rule_windows(rule, &state);
    }
    let _ = startup::set_enabled(true);
    Ok(())
}

#[tauri::command]
fn exit_app(app: tauri::AppHandle, state: State<'_, AppState>) {
    state.restore_before_exit();
    app.exit(0);
}

#[tauri::command]
fn restart_as_admin(app: AppHandle) -> Result<(), String> {
    use windows::{
        Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_HIDE},
        core::PCWSTR,
    };
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let executable: Vec<u16> = executable
        .as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let operation: Vec<u16> = "runas\0".encode_utf16().collect();
    let parameters: Vec<u16> = "--replace --background\0".encode_utf16().collect();
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(operation.as_ptr()),
            PCWSTR(executable.as_ptr()),
            PCWSTR(parameters.as_ptr()),
            None,
            SW_HIDE,
        )
    };
    if result.0 as isize <= 32 {
        return Err(format!("elevation request failed: {}", result.0 as isize));
    }
    if let Some(state) = app.try_state::<AppState>() {
        state.restore_before_exit();
    }
    app.exit(0);
    Ok(())
}

#[tauri::command]
fn minimize_window(window: tauri::WebviewWindow) {
    let _ = window.minimize();
}

#[tauri::command]
fn close_window(window: tauri::WebviewWindow) {
    let _ = window.hide();
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
    let updater = app.updater().map_err(|error| error.to_string())?;
    match updater.check().await.map_err(|error| error.to_string())? {
        Some(update) => {
            update
                .download_and_install(|_, _| {}, || {})
                .await
                .map_err(|error| error.to_string())?;
            if let Some(state) = app.try_state::<AppState>() {
                state.restore_before_exit();
            }
            app.restart()
        }
        None => Ok("LATEST".to_owned()),
    }
}

async fn install_background_update(app: tauri::AppHandle) {
    let Ok(updater) = app.updater() else {
        return;
    };
    let Ok(Some(update)) = updater.check().await else {
        return;
    };
    if update.download_and_install(|_, _| {}, || {}).await.is_ok() {
        if let Some(state) = app.try_state::<AppState>() {
            state.restore_before_exit();
        }
        app.restart();
    }
}

pub fn run(instance: SingleInstance) -> anyhow::Result<()> {
    std::mem::forget(instance);
    let quiet = std::env::args().any(|arg| matches!(arg.as_str(), "--autostart" | "--background"));
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |app| {
            let _ = startup::ensure_elevated_task();
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_decorations(false);
                let _ = window.set_shadow(false);
                let scale = window.scale_factor().unwrap_or(1.0);
                let _ = window.set_size(tauri::PhysicalSize::new(
                    (488.0 * scale).round() as u32,
                    (720.0 * scale).round() as u32,
                ));
            }
            let config = Arc::new(RwLock::new(Config::load()));
            let _ = startup::set_enabled(config.read().unwrap().startup_enabled);
            let statuses = Arc::new(RwLock::new(HashMap::new()));
            let controller = WindowController::default();
            let watcher = Watcher::start(config.clone(), statuses.clone(), controller.clone());
            app.manage(AppState {
                config,
                statuses,
                controller,
                _watcher: watcher,
                icon_cache: Mutex::new(HashMap::new()),
            });

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
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => {
                        if let Some(state) = app.try_state::<AppState>() {
                            state.restore_before_exit();
                        }
                        app.exit(0);
                    }
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        ..
                    } = event
                    {
                        if let Some(window) = tray.app_handle().get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;
            if quiet {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            list_windows,
            add_application,
            remove_application,
            set_automatic,
            set_language,
            set_theme,
            set_startup,
            reset_settings,
            exit_app,
            restart_as_admin,
            minimize_window,
            close_window,
            start_window_drag,
            set_window_height,
            get_app_version,
            check_for_updates_manual
        ])
        .run(tauri::generate_context!())
        .map_err(|error| anyhow::anyhow!(error.to_string()))
}
