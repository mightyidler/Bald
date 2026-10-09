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
    config: Arc<RwLock<Config>>,
    service: Arc<Mutex<BorderService>>,
    icon_cache: Mutex<HashMap<String, String>>,
}

impl AppState {
    fn restore_before_exit(&self) {
        if let Err(error) = self.service.lock().unwrap().shutdown() {
            crate::diagnostics::record(format!("exit_restore_failed {error}"));
            eprintln!("{error}");
        }
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
async fn get_state(app: AppHandle) -> Result<UiState, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        read_ui_state(&state)
    })
    .await
    .map_err(|error| error.to_string())?
}

fn read_ui_state(state: &AppState) -> Result<UiState, String> {
    let config = state.config.read().unwrap().clone();
    let statuses = state
        .service
        .lock()
        .unwrap()
        .statuses()
        .map_err(|error| error.to_string())?
        .iter()
        .map(|(id, value)| (id.to_string(), status_name(value).to_owned()))
        .collect();
    let application_icons = config
        .applications
        .iter()
        .filter_map(|rule| {
            cached_icon_data_url(state, rule.executable_path.as_deref())
                .map(|icon| (rule.id.to_string(), icon))
        })
        .collect();
    Ok(UiState {
        config,
        statuses,
        application_icons,
        startup_enabled: state.config.read().unwrap().startup_enabled,
    })
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

async fn persist(state: &AppState) -> Result<(), String> {
    let saved = state
        .config
        .read()
        .unwrap()
        .save()
        .map_err(|error| error.to_string());
    let config = state.config.clone();
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut service = service.lock().unwrap();
        let snapshot = config.read().unwrap().clone();
        service.sync(snapshot)
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())?;
    saved
}

async fn restore_in_background(
    service: Arc<Mutex<BorderService>>,
    config: Arc<RwLock<Config>>,
    rule: Option<ApplicationRule>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut service = service.lock().unwrap();
        let snapshot = config.read().unwrap().clone();
        service.sync(snapshot)?;
        service.restore(rule)
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

async fn restore_rule_windows(rule: &ApplicationRule, state: &AppState) -> Result<(), String> {
    let config = state.config.clone();
    restore_in_background(state.service.clone(), config, Some(rule.clone())).await
}

#[tauri::command]
async fn add_application(choice: WindowChoice, state: State<'_, AppState>) -> Result<(), String> {
    let window = enumerate_windows()
        .into_iter()
        .find(|window| {
            window.title == choice.title && window.executable_name == choice.executable_name
        })
        .ok_or_else(|| "window is no longer available".to_owned())?;
    {
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
    }
    persist(&state).await
}

#[tauri::command]
async fn remove_application(id: String, state: State<'_, AppState>) -> Result<(), String> {
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
    let restored = if let Some(rule) = rule.as_ref() {
        restore_rule_windows(rule, &state).await
    } else {
        Ok(())
    };
    let saved = persist(&state).await;
    restored.and(saved)
}

#[tauri::command]
async fn set_application_drag_mode(
    id: String,
    mode: DragMode,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let id = Uuid::parse_str(&id).map_err(|error| error.to_string())?;
    let config = state.config.clone();
    let service = state.service.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        // Serialize configuration and native requests off the WebView thread.
        let mut service = service.lock().unwrap();
        let mut config = config.write().unwrap();
        let rule = config
            .applications
            .iter_mut()
            .find(|rule| rule.id == id)
            .ok_or_else(|| "application no longer registered".to_owned())?;
        rule.drag_mode = mode;
        let rule = rule.clone();
        config.save().map_err(|error| error.to_string())?;
        let snapshot = config.clone();
        drop(config);
        service.sync(snapshot).map_err(|error| error.to_string())?;
        service.drag(rule).map_err(|error| error.to_string())?;
        Ok(())
    })
    .await
    .map_err(|error| error.to_string())?;
    result
}

#[tauri::command]
async fn set_automatic(enabled: bool, state: State<'_, AppState>) -> Result<(), String> {
    state.config.write().unwrap().automatic_application = enabled;
    let restored = if !enabled {
        let config = state.config.clone();
        restore_in_background(state.service.clone(), config, None).await
    } else {
        Ok(())
    };
    let saved = persist(&state).await;
    restored.and(saved)
}

#[tauri::command]
async fn set_language(language: Language, state: State<'_, AppState>) -> Result<(), String> {
    state.config.write().unwrap().language = language;
    persist(&state).await
}

#[tauri::command]
async fn set_theme(theme: String, state: State<'_, AppState>) -> Result<(), String> {
    if !matches!(theme.as_str(), "system" | "light" | "dark") {
        return Err("unsupported theme".to_owned());
    }
    state.config.write().unwrap().theme = theme;
    persist(&state).await
}

#[tauri::command]
async fn set_startup(enabled: bool, state: State<'_, AppState>) -> Result<(), String> {
    startup::set_enabled(enabled).map_err(|error| error.to_string())?;
    state.config.write().unwrap().startup_enabled = enabled;
    persist(&state).await
}

#[tauri::command]
async fn reset_settings(state: State<'_, AppState>) -> Result<(), String> {
    *state.config.write().unwrap() = Config::default();
    let config = state.config.clone();
    let restored = restore_in_background(state.service.clone(), config, None).await;
    let saved = persist(&state).await;
    let startup = startup::set_enabled(true).map_err(|error| error.to_string());
    restored.and(saved).and(startup)
}

#[tauri::command]
fn exit_app(app: tauri::AppHandle, state: State<'_, AppState>) {
    state.restore_before_exit();
    app.exit(0);
}

#[tauri::command]
async fn elevate_border_service(state: State<'_, AppState>) -> Result<(), String> {
    let service = state.service.clone();
    let config = state.config.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut service = service.lock().unwrap();
        let snapshot = config.read().unwrap().clone();
        service.elevate(snapshot)
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

fn capture_ui_state(
    app: &AppHandle,
) -> Result<Option<crate::ui_window_state::UiWindowState>, String> {
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

fn restart_preserving_ui(app: &AppHandle) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let arguments = restart_arguments(capture_ui_state(app)?)?;
    std::process::Command::new(std::env::current_exe().map_err(|error| error.to_string())?)
        .args(arguments)
        .creation_flags(0x0800_0000)
        .spawn()
        .map_err(|error| error.to_string())?;
    if let Some(state) = app.try_state::<AppState>() {
        state.restore_before_exit();
    }
    app.exit(0);
    Ok(())
}

#[tauri::command]
fn minimize_window(window: tauri::WebviewWindow) -> Result<(), String> {
    sync_visible_window(&window)?;
    window.minimize().map_err(|error| error.to_string())
}

#[tauri::command]
fn close_window(window: tauri::WebviewWindow) -> Result<(), String> {
    sync_visible_window(&window)?;
    window.hide().map_err(|error| error.to_string())?;
    Ok(())
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
    let updater = app.updater().map_err(|error| error.to_string())?;
    match updater.check().await.map_err(|error| error.to_string())? {
        Some(update) => {
            update
                .download_and_install(|_, _| {}, || {})
                .await
                .map_err(|error| error.to_string())?;
            restart_preserving_ui(&app)?;
            Ok("INSTALLED".to_owned())
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
    if update.download_and_install(|_, _| {}, || {}).await.is_ok()
        && let Err(error) = restart_preserving_ui(&app)
    {
        eprintln!("update restart failed: {error}");
    }
}

fn open_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let replay_content = !window.is_visible().unwrap_or(true)
            && !window.is_minimized().unwrap_or(true);
        if replay_content {
            let _ = window.eval("window.dispatchEvent(new Event('bald-window-reopened'))");
        }
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

pub fn run(instance: SingleInstance) -> anyhow::Result<()> {
    std::mem::forget(instance);
    let quiet = std::env::args().any(|arg| matches!(arg.as_str(), "--autostart" | "--background"));
    let restore_visible = std::env::args().any(|arg| arg == "--restore-visible");
    let restore_minimized = std::env::args().any(|arg| arg == "--restore-minimized");
    let restored_ui =
        std::env::args().find_map(|arg| crate::ui_window_state::UiWindowState::from_argument(&arg));
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |app| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_decorations(false);
                let _ = window.set_shadow(false);
                let native = window.hwnd()?;
                crate::own_window_frame::install(windows::Win32::Foundation::HWND(native.0))?;
                let scale = window.scale_factor().unwrap_or(1.0);
                let _ = window.set_size(tauri::PhysicalSize::new(
                    (488.0 * scale).round() as u32,
                    (720.0 * scale).round() as u32,
                ));
            }
            let config = Arc::new(RwLock::new(Config::load()));
            if !crate::diagnostics::enabled() {
                let _ = startup::set_enabled(config.read().unwrap().startup_enabled);
            }
            let service = Arc::new(Mutex::new(BorderService::new(
                config.read().unwrap().clone(),
            )));
            app.manage(AppState {
                config,
                service,
                icon_cache: Mutex::new(HashMap::new()),
            });

            let updater_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if !crate::diagnostics::enabled() {
                    install_background_update(updater_app).await;
                }
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
                        if let Some(state) = app.try_state::<AppState>() {
                            state.restore_before_exit();
                        }
                        app.exit(0);
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
                if let Some(window) = app.get_webview_window("main") {
                    let hwnd = window.hwnd()?;
                    snapshot.apply(windows::Win32::Foundation::HWND(hwnd.0))?;
                }
            } else if restore_visible || restore_minimized {
                if let Some(window) = app.get_webview_window("main") {
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
            } else if quiet {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
            } else if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if let Some(main) = window.app_handle().get_webview_window("main") {
                    let _ = close_window(main);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
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
            if matches!(
                event,
                tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
            ) {
                if let Some(state) = app.try_state::<AppState>() {
                    state.restore_before_exit();
                }
            }
        });
    Ok(())
}
