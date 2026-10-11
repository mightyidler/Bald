param(
    [Parameter(Mandatory)][string]$SourceRoot,
    [ValidateSet('before', 'after', 'lazy', 'validation')][string]$Name
)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$source = (Resolve-Path -LiteralPath $SourceRoot).Path
$destination = Join-Path $repo "artifacts/performance-2026-10-10/$Name"
if (Test-Path -LiteralPath $destination) { throw "Probe directory already exists: $destination" }
New-Item -ItemType Directory -Path $destination | Out-Null
foreach ($entry in @('src','web','icons','assets','tools','Cargo.toml','Cargo.lock','build.rs','tauri.conf.json')) {
    Copy-Item -LiteralPath (Join-Path $source $entry) -Destination $destination -Recurse
}
if (Test-Path (Join-Path $source 'capabilities')) { Copy-Item -LiteralPath (Join-Path $source 'capabilities') -Destination $destination -Recurse }
function Write-ProbeText([string]$relative, [string]$value) {
    [IO.File]::WriteAllText((Join-Path $destination $relative), $value, [Text.UTF8Encoding]::new($false))
}
$id = [guid]::NewGuid().ToString('N')
$settings = Get-Content (Join-Path $destination 'tauri.conf.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$settings.identifier = "app.bald.probe.$id"
$settings.productName = "Bald Probe $Name"
$settings.app.windows[0].title = "Bald Probe $Name"
$settings.app.windows[0].center = $false
$settings.app.windows[0] | Add-Member -NotePropertyName x -NotePropertyValue (-24000.0)
$settings.app.windows[0] | Add-Member -NotePropertyName y -NotePropertyValue (-24000.0)
if ($Name -eq 'lazy') { $settings.app.windows[0] | Add-Member -NotePropertyName create -NotePropertyValue $false -Force }
Write-ProbeText 'tauri.conf.json' ($settings | ConvertTo-Json -Depth 30)
$config = Get-Content (Join-Path $destination 'src/config.rs') -Raw -Encoding UTF8
$load = @'
    pub fn load() -> Self {
        let mut config = Self { automatic_application: false, startup_enabled: false, ..Self::default() };
        let count = std::env::args().find_map(|arg| arg.strip_prefix("--probe-rules=").and_then(|n| n.parse::<usize>().ok())).unwrap_or(0).min(50);
        for index in 0..count {
            config.applications.push(crate::rules::ApplicationRule {
                id: uuid::Uuid::new_v4(), display_name: format!("Probe fixture {index}"),
                executable_name: "BaldProbeTarget.exe".into(),
                executable_path: std::env::current_exe().ok().map(|path| path.to_string_lossy().into_owned()),
                window_class_hint: Some("BaldProbeMissingClass".into()), enabled: true,
                drag_mode: crate::rules::DragMode::Disabled,
            });
        }
        config
    }

'@
$config = [regex]::Replace($config, '(?s)    pub fn load\(\) -> Self \{.*?(?=    pub fn save)', [Text.RegularExpressions.MatchEvaluator]{param($m) $load})
$path = (Join-Path $destination 'probe-config.json')
$config = [regex]::Replace($config, '(?s)pub fn config_path\(\) -> PathBuf \{.*?(?=fn migrate_legacy_config)', [Text.RegularExpressions.MatchEvaluator]{param($m) "pub fn config_path() -> PathBuf { PathBuf::from(r`"$path`") }`n`n"})
Write-ProbeText 'src/config.rs' $config
$startup = Get-Content (Join-Path $destination 'src/startup.rs') -Raw -Encoding UTF8
Write-ProbeText 'src/startup.rs' ($startup.Replace('cfg!(debug_assertions)', 'true'))
$instance = Get-Content (Join-Path $destination 'src/single_instance.rs') -Raw -Encoding UTF8
Write-ProbeText 'src/single_instance.rs' ($instance.Replace('Bald.SingleInstance.v1', "Bald.Probe.$id").Replace('"Bald\0"', '"Bald Probe Missing Legacy\0"'))
if (Test-Path (Join-Path $destination 'src/ui_activation.rs')) {
    $activation = Get-Content (Join-Path $destination 'src/ui_activation.rs') -Raw -Encoding UTF8
    Write-ProbeText 'src/ui_activation.rs' ($activation.Replace('Bald.Activation.v1', "Bald.Probe.Activation.$id"))
}
$main = Get-Content (Join-Path $destination 'src/main.rs') -Raw -Encoding UTF8
Write-ProbeText 'src/main.rs' ($main.Replace('mod web_app;', "mod web_app;`nmod performance_probe;"))
# Never allow an unattended probe to request elevation, even on an unexpected match.
$service = Get-Content (Join-Path $destination 'src/border_service.rs') -Raw -Encoding UTF8
$service = [regex]::Replace($service, '(?s)    fn launch\(\) -> Result<Self> \{.*?\n    \}\n(?=\})', '    fn launch() -> Result<Self> { bail!("UAC is disabled in performance probes"); }' + "`n")
if ($service.Contains('ShellExecuteExW') -or !$service.Contains('UAC is disabled in performance probes')) { throw 'Could not remove UAC launch from the probe.' }
Write-ProbeText 'src/border_service.rs' $service
$native = Get-Content (Join-Path $destination 'src/window_manager.rs') -Raw -Encoding UTF8
$native = $native.Replace('pub fn enumerate_windows() -> Vec<WindowInfo> {', 'pub fn enumerate_windows() -> Vec<WindowInfo> { crate::performance_probe::ENUMS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);')
$native = [regex]::Replace($native, '(fn process_path\([^{}]+\) -> Option<(?:String|ProcessPath)> \{)', '$1 crate::performance_probe::PATHS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);')
$native = $native.Replace('fn process_is_elevated(process: HANDLE) -> Option<bool> {', 'fn process_is_elevated(process: HANDLE) -> Option<bool> { crate::performance_probe::TOKENS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);')
Write-ProbeText 'src/window_manager.rs' $native
$watcher = Get-Content (Join-Path $destination 'src/watcher.rs') -Raw -Encoding UTF8
$watcher = [regex]::Replace($watcher, '(?s)(fn scan\([^{}]*\)[^{]*\{)', '$1 crate::performance_probe::SCANS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);')
Write-ProbeText 'src/watcher.rs' $watcher
$web = Get-Content (Join-Path $destination 'src/web_app.rs') -Raw -Encoding UTF8
# Compile out both update paths in the copy, rather than relying on network failure.
$web = [regex]::Replace($web, '(?s)async fn check_for_updates_manual\(.*?(?=async fn install_background_update)', 'async fn check_for_updates_manual(_app: tauri::AppHandle) -> Result<String, String> { Ok("LATEST".into()) }' + "`n`n")
$web = [regex]::Replace($web, '(?s)async fn install_background_update\(.*?(?=(?:fn create_main_window|fn open_main_window|pub\(crate\) fn open_main_window))', 'async fn install_background_update(_app: tauri::AppHandle) {}' + "`n`n")
$realLifecycle = $web.Contains('fn create_main_window(')
if ($Name -eq 'validation') {
    $web = $web.Replace('let window = tauri::WebviewWindowBuilder::from_config(app, config)?.build()?;', 'crate::performance_probe::note("ui-create-start".into(), "settings".into()); let window = tauri::WebviewWindowBuilder::from_config(app, config)?.build()?;')
}
$web = $web.Replace('fn create_main_window(', 'pub(crate) fn create_main_window(').Replace('fn close_window(', 'pub(crate) fn close_window(').Replace('fn minimize_window(', 'pub(crate) fn minimize_window(')
$web = $web.Replace('let _ = window.set_focus();', 'let _ = &window;')
$web = $web.Replace('let updater_app = app.handle().clone();', 'crate::performance_probe::start(app.handle().clone());' + "`n            let updater_app = app.handle().clone();")
$web = $web.Replace('tauri::generate_handler![', 'tauri::generate_handler![crate::performance_probe::note,')
$web = [regex]::Replace($web, '(?s)\.run\(\|app, event\| \{.*?\}\);(?=\s*Ok\(\(\)\))', '.run(|_, event| { if let tauri::RunEvent::ExitRequested { code: None, api, .. } = event { api.prevent_exit(); } });')
Write-ProbeText 'src/web_app.rs' $web
$probe = @'
use std::{io::Write, sync::Mutex, time::Instant};
use tauri::{AppHandle, Manager};
static LOG: Mutex<()> = Mutex::new(());
static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
static PREFERENCE_ACKS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub static SCANS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static ENUMS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static PATHS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static TOKENS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
#[tauri::command]
pub fn note(kind: String, data: String) {
    if kind == "restored-preferences" { PREFERENCE_ACKS.fetch_add(1, std::sync::atomic::Ordering::Release); }
    if matches!(kind.as_str(), "error" | "js-error" | "reported-error" | "state-error") { FAILED.store(true, std::sync::atomic::Ordering::Release); }
    let _guard = LOG.lock().unwrap();
    let path = std::env::current_exe().unwrap().with_extension("events.jsonl");
    let value = serde_json::json!({"seconds": START.get_or_init(Instant::now).elapsed().as_secs_f64(), "kind": kind, "data": data});
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) { let _ = writeln!(file, "{value}"); }
}
pub fn start(app: AppHandle) {
    START.get_or_init(Instant::now);
    if std::env::args().any(|arg| arg == "--probe-lifecycle") { lifecycle(app); return; }
    note("phase".into(), "background".into());
    std::thread::spawn(move || {
        for (delay, phase) in [(15, "shown"), (20, "minimized"), (20, "hidden"), (20, "reopened"), (20, "closed"), (15, "recreated"), (15, "closed-again")] {
            std::thread::sleep(std::time::Duration::from_secs(delay));
            let next = app.clone();
            let (sent, done) = std::sync::mpsc::channel();
            let _ = app.run_on_main_thread(move || {
                let started = Instant::now();
                if matches!(phase, "shown" | "reopened" | "recreated") {
                    if next.get_webview_window("main").is_none() {
                        let config = next.config().app.windows.iter().find(|w| w.label == "main").unwrap();
                        match tauri::WebviewWindowBuilder::from_config(&next, config).and_then(|builder| builder.build()) {
                          Ok(window) => {
                            if let Ok(hwnd) = window.hwnd() { let _ = crate::own_window_frame::install(windows::Win32::Foundation::HWND(hwnd.0)); }
                          },
                          Err(error) => note("error".into(), error.to_string()),
                        }
                    }
                }
                if let Some(window) = next.get_webview_window("main") {
                    if matches!(phase, "closed" | "closed-again") && __LAZY__ {
                        let _ = window.destroy();
                    } else if let Ok(hwnd) = window.hwnd() {
                        use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_SHOWNOACTIVATE, SW_SHOWMINNOACTIVE, SW_HIDE};
                        unsafe { let _ = ShowWindow(windows::Win32::Foundation::HWND(hwnd.0), match phase { "shown" | "reopened" | "recreated" => SW_SHOWNOACTIVATE, "minimized" => SW_SHOWMINNOACTIVE, _ => SW_HIDE }); }
                        let _ = window.eval(&format!("window.dispatchEvent(new CustomEvent('bald-probe-visible', {{detail:{}}}));", matches!(phase, "shown" | "reopened" | "recreated")));
                    }
                    if phase == "shown" {
                        let _ = window.eval("setTimeout(()=>{const t=performance.now();window.__TAURI__.core.invoke('list_windows').then(v=>window.__TAURI__.core.invoke('note',{kind:'picker',data:JSON.stringify({ms:performance.now()-t,count:v.length})}));},3000)");
                    }
                }
                note("phase".into(), phase.into());
                note("native-action-ms".into(), started.elapsed().as_secs_f64().mul_add(1000.0,0.0).to_string());
                use std::sync::atomic::Ordering::Relaxed;
                note("counts".into(), serde_json::json!({"scans": SCANS.load(Relaxed), "enumWindows": ENUMS.load(Relaxed), "processPaths": PATHS.load(Relaxed), "tokenQueries": TOKENS.load(Relaxed)}).to_string());
                let _ = sent.send(());
            });
            if done.recv_timeout(std::time::Duration::from_secs(10)).is_err() { note("error".into(), "main thread did not respond".into()); break; }
        }
        std::thread::sleep(std::time::Duration::from_secs(15));
        app.exit(0);
    });
}
'@
if ($realLifecycle) {
    $probe = [regex]::Replace($probe, '(?s)if next.get_webview_window\("main"\).is_none\(\) \{.*?(?=\n                \}\n                if let Some)', 'if let Err(error) = crate::web_app::create_main_window(&next) { note("error".into(), error.to_string()); }')
    $probe = $probe.Replace('let _ = window.destroy();', 'if let Err(error) = crate::web_app::close_window(window.clone()) { note("error".into(), error); }')
    $probe += @'

fn lifecycle(app: AppHandle) {
    std::thread::spawn(move || {
        let tested = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            assert!(app.get_webview_window("main").is_none(), "background constructed a WebView");
            let secondary = || {
                let status = std::process::Command::new(std::env::current_exe().unwrap()).status().unwrap();
                assert!(status.success());
                let started = Instant::now();
                while app.get_webview_window("main").is_none() && started.elapsed().as_secs() < 10 { std::thread::sleep(std::time::Duration::from_millis(20)); }
                assert!(app.get_webview_window("main").is_some(), "second instance failed to create UI");
            };
            secondary();
            std::thread::sleep(std::time::Duration::from_secs(3));
            let next = app.clone();
            let (send, receive) = std::sync::mpsc::channel();
            app.run_on_main_thread(move || {
                let window = next.get_webview_window("main").unwrap();
                window.set_position(tauri::PhysicalPosition::new(170, 190)).unwrap();
                window.eval("Promise.all([invoke('set_theme',{theme:'light'}),invoke('set_language',{language:'english'})]).then(()=>invoke('get_state').then(s=>probeInvoke('note',{kind:'preferences',data:JSON.stringify(s.config)}))).catch(e=>probeInvoke('note',{kind:'error',data:String(e)}))").unwrap();
                send.send(()).unwrap();
            }).unwrap();
            receive.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
            std::thread::sleep(std::time::Duration::from_secs(2));
            let first = app.get_webview_window("main").unwrap();
            let expected = (first.outer_position().unwrap(), first.outer_size().unwrap());
            drop(first);
            for cycle in 0..6 {
                let next = app.clone();
                let (send, receive) = std::sync::mpsc::channel();
                app.run_on_main_thread(move || {
                    let window = next.get_webview_window("main").unwrap();
                    if cycle == 2 { crate::web_app::minimize_window(window.clone()).unwrap(); }
                    if cycle == 4 { window.close().unwrap(); }
                    else if cycle == 5 { window.eval("document.querySelector('#closeBtn').click()").unwrap(); }
                    else { crate::web_app::close_window(window).unwrap(); }
                    if cycle == 3 { crate::web_app::open_main_window(&next); }
                    send.send(()).unwrap();
                }).unwrap();
                receive.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
                std::thread::sleep(std::time::Duration::from_millis(400));
                if cycle != 3 {
                    assert!(app.get_webview_window("main").is_none(), "closed WebView remained registered");
                    secondary();
                }
                std::thread::sleep(std::time::Duration::from_secs(2));
                let window = app.get_webview_window("main").expect("reopen did not complete");
                assert_eq!(window.outer_position().unwrap(), expected.0, "position changed on cycle {cycle}");
                assert_eq!(window.outer_size().unwrap(), expected.1, "size changed on cycle {cycle}");
                window.eval("invoke('get_state').then(s=>{if(s.config.theme!=='light'||s.config.language!=='english')throw Error('preferences lost');probeInvoke('note',{kind:'restored-preferences',data:s.config.theme+':'+s.config.language})}).catch(e=>probeInvoke('note',{kind:'error',data:String(e)}))").unwrap();
                let ready = Instant::now();
                while PREFERENCE_ACKS.load(std::sync::atomic::Ordering::Acquire) <= cycle && ready.elapsed().as_secs() < 10 { std::thread::sleep(std::time::Duration::from_millis(20)); }
                assert!(PREFERENCE_ACKS.load(std::sync::atomic::Ordering::Acquire) > cycle, "preference check did not complete on cycle {cycle}");
                note("lifecycle-cycle".into(), cycle.to_string());
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
            assert!(SCANS.load(std::sync::atomic::Ordering::Relaxed) > 2, "watcher stopped while UI was destroyed");
            assert!(!FAILED.load(std::sync::atomic::Ordering::Acquire), "UI reported an asynchronous failure");
        }));
        if tested.is_err() { note("error".into(), "lifecycle assertion failed".into()); app.exit(1); }
        else { note("lifecycle-result".into(), "passed".into()); app.exit(0); }
    });
}
'@
} else {
    $probe += "`nfn lifecycle(app: AppHandle) { app.exit(1); }`n"
}
if ($Name -eq 'validation') {
    $probe = $probe.Replace('pub fn start(app: AppHandle) {', 'pub fn start(app: AppHandle) { if std::env::args().any(|arg| arg == "--probe-tray") { tray_lifecycle(app); return; }')
    $probe += @'

fn tray_lifecycle(app: AppHandle) {
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            use windows::Win32::{Foundation::{HWND, LPARAM, WPARAM}, UI::WindowsAndMessaging::{EnumWindows, GetClassNameW, GetWindowThreadProcessId, PostMessageW, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_LBUTTONDBLCLK}};
            unsafe extern "system" fn find(hwnd: HWND, data: LPARAM) -> windows::core::BOOL {
                unsafe {
                    let mut pid = 0; GetWindowThreadProcessId(hwnd, Some(&mut pid));
                    if pid != std::process::id() { return true.into(); }
                    let mut name = [0u16; 64]; let length = GetClassNameW(hwnd, &mut name) as usize;
                    if String::from_utf16_lossy(&name[..length]) == "tray_icon_app" {
                        *(data.0 as *mut isize) = hwnd.0 as isize; return false.into();
                    }
                    true.into()
                }
            }
            let mut handle = 0isize; let started = Instant::now();
            while handle == 0 && started.elapsed().as_secs() < 5 {
                let _ = unsafe { EnumWindows(Some(find), LPARAM((&mut handle as *mut isize) as isize)) };
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            assert_ne!(handle, 0, "owned native tray window was not created");
            let tray = HWND(handle as *mut _);
            for cycle in 0..3 {
                assert!(app.get_webview_window("main").is_none());
                // tray-icon 0.25.1's notification message; only this process's
                // tray window receives it. No cursor movement or SendInput.
                for event in [WM_LBUTTONDOWN, WM_LBUTTONUP] { unsafe { PostMessageW(Some(tray), 6002, WPARAM(0), LPARAM(event as isize)).unwrap(); } }
                std::thread::sleep(std::time::Duration::from_millis(250));
                assert!(app.get_webview_window("main").is_none(), "single click opened settings");
                unsafe { PostMessageW(Some(tray), 6002, WPARAM(0), LPARAM(WM_LBUTTONDBLCLK as isize)).unwrap(); }
                let started = Instant::now();
                while app.get_webview_window("main").is_none() && started.elapsed().as_secs() < 10 { std::thread::sleep(std::time::Duration::from_millis(20)); }
                assert!(app.get_webview_window("main").is_some(), "tray double click did not open settings");
                std::thread::sleep(std::time::Duration::from_secs(2));
                let queued = app.clone(); let (sent, done) = std::sync::mpsc::channel();
                app.run_on_main_thread(move || { crate::web_app::close_window(queued.get_webview_window("main").unwrap()).unwrap(); sent.send(()).unwrap(); }).unwrap();
                done.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
                std::thread::sleep(std::time::Duration::from_millis(400));
                assert!(app.get_webview_window("main").is_none(), "tray-opened WebView did not release");
                note("tray-cycle".into(), cycle.to_string());
            }
            assert!(!FAILED.load(std::sync::atomic::Ordering::Acquire));
        }));
        if result.is_err() { note("error".into(), "tray assertion failed".into()); app.exit(1); }
        else { note("tray-result".into(), "passed".into()); app.exit(0); }
    });
}
'@
}
Write-ProbeText 'src/performance_probe.rs' ($probe.Replace('__LAZY__', ($Name -eq 'lazy' -or $realLifecycle).ToString().ToLowerInvariant()))
$js = Get-Content (Join-Path $destination 'web/app.js') -Raw -Encoding UTF8
if ($Name -eq 'validation') {
    $js = $js.Replace('stack.inert = false;', 'stack.inert = false; probeInvoke("note",{kind:"interactive-ready",data:String(performance.now())});')
}
$instrument = @'
const probeInvoke=window.__TAURI__.core.invoke;
window.addEventListener('error', event=>probeInvoke('note',{kind:'js-error',data:String(event.error||event.message)}));
const probeTrackedInvoke=(command,args)=>{
  const started=performance.now(),request=probeInvoke(command,args);
  if(command==='get_state')request.then(value=>probeInvoke('note',{kind:'state',data:JSON.stringify({bytes:new TextEncoder().encode(JSON.stringify(value)).length,ms:performance.now()-started})})).catch(error=>probeInvoke('note',{kind:'state-error',data:String(error)}));
  return request;
};
'@
$js = $instrument + "`n" + $js.Replace('const invoke=window.__TAURI__.core.invoke;', 'const invoke=probeTrackedInvoke;').Replace('function reportWindowError(error){', "function reportWindowError(error){probeInvoke('note',{kind:'reported-error',data:String(error)});") + "`nwindow.addEventListener('bald-probe-visible', event=>{"
if ($Name -ne 'before') { $js += "setUiVisibility(event.detail);" }
$js += "probeInvoke('note',{kind:'visible-ready',data:String(event.detail)});});`nprobeInvoke('note',{kind:'page-ready',data:String(performance.now())});`n"
Write-ProbeText 'web/app.js' $js
& node --check (Join-Path $destination 'web/app.js')
if ($LASTEXITCODE) { throw 'The generated probe JavaScript failed syntax validation.' }
Write-Output $destination
