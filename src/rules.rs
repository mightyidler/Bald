use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::window_manager::WindowInfo;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DragMode {
    Enabled,
    #[default]
    #[serde(alias = "auto")]
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplicationRule {
    pub id: Uuid,
    pub display_name: String,
    pub executable_name: String,
    pub executable_path: Option<String>,
    pub window_class_hint: Option<String>,
    pub enabled: bool,
    #[serde(default)]
    pub drag_mode: DragMode,
}

impl ApplicationRule {
    pub fn from_window(window: &WindowInfo) -> Self {
        Self {
            id: Uuid::new_v4(),
            display_name: window.title.clone(),
            executable_name: window.executable_name.clone(),
            executable_path: window.executable_path.clone(),
            window_class_hint: (!window.class_name.is_empty()).then(|| window.class_name.clone()),
            enabled: true,
            drag_mode: DragMode::Disabled,
        }
    }

    pub fn matches(&self, window: &WindowInfo) -> bool {
        if let (Some(expected), Some(actual)) = (&self.executable_path, &window.executable_path) {
            return normalize_path(expected) == normalize_path(actual);
        }
        self.executable_name
            .eq_ignore_ascii_case(&window.executable_name)
            && self.class_matches_when_needed(window)
    }

    // An exact executable path may also own splash/launcher windows. Automatic
    // application uses the registered class; manual restore keeps process scope.
    pub fn matches_automatic_window(&self, window: &WindowInfo) -> bool {
        self.matches(window) && self.class_matches_when_needed(window)
    }

    fn class_matches_when_needed(&self, window: &WindowInfo) -> bool {
        self.window_class_hint
            .as_ref()
            .map(|hint| hint.eq_ignore_ascii_case(&window.class_name))
            .unwrap_or(true)
    }

    pub fn duplicates(&self, window: &WindowInfo) -> bool {
        match (&self.executable_path, &window.executable_path) {
            (Some(a), Some(b)) => normalize_path(a) == normalize_path(b),
            _ => self
                .executable_name
                .eq_ignore_ascii_case(&window.executable_name),
        }
    }
}

fn normalize_path(path: &str) -> String {
    #[cfg(test)]
    tests::NORMALIZATION_CALLS.with(|calls| calls.set(calls.get() + 1));
    path.replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

pub(crate) fn matching_windows<'a>(
    rules: &[ApplicationRule],
    windows: &'a [WindowInfo],
) -> Vec<Vec<&'a WindowInfo>> {
    let paths: Vec<_> = windows
        .iter()
        .map(|window| window.executable_path.as_deref().map(normalize_path))
        .collect();
    rules
        .iter()
        .map(|rule| {
            let expected = rule.executable_path.as_deref().map(normalize_path);
            windows
                .iter()
                .zip(&paths)
                .filter_map(|(window, actual)| {
                    let executable_matches = match (&expected, actual) {
                        (Some(expected), Some(actual)) => expected == actual,
                        _ => rule
                            .executable_name
                            .eq_ignore_ascii_case(&window.executable_name),
                    };
                    (executable_matches && rule.class_matches_when_needed(window)).then_some(window)
                })
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    std::thread_local! {
        pub(super) static NORMALIZATION_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    #[test]
    fn scan_normalizes_each_rule_and_window_once_for_identical_inputs() {
        let windows: Vec<_> = (0..100)
            .map(|index| window(Some(&format!("c:/games/{index}.exe")), "Game.exe", "Main"))
            .collect();
        let rules: Vec<_> = windows[..50]
            .iter()
            .map(ApplicationRule::from_window)
            .collect();
        NORMALIZATION_CALLS.with(|calls| calls.set(0));
        let shared = matching_windows(&rules, &windows);
        assert_eq!(NORMALIZATION_CALLS.with(|calls| calls.get()), 150);
        NORMALIZATION_CALLS.with(|calls| calls.set(0));
        for _ in 0..3 {
            let repeated: Vec<Vec<_>> = rules
                .iter()
                .map(|rule| {
                    windows
                        .iter()
                        .filter(|window| rule.matches_automatic_window(window))
                        .collect()
                })
                .collect();
            assert_eq!(shared.len(), repeated.len());
            for (shared, repeated) in shared.iter().zip(repeated) {
                assert_eq!(shared.len(), repeated.len());
                assert!(
                    shared
                        .iter()
                        .zip(repeated)
                        .all(|(a, b)| std::ptr::eq(*a, b))
                );
            }
        }
        assert_eq!(NORMALIZATION_CALLS.with(|calls| calls.get()), 30000);
    }

    fn window(path: Option<&str>, name: &str, class: &str) -> WindowInfo {
        WindowInfo {
            hwnd: 1,
            process_id: 1,
            title: "Example".into(),
            executable_name: name.into(),
            executable_path: path.map(str::to_owned),
            class_name: class.into(),
            is_borderless: false,
        }
    }

    #[test]
    fn old_automatic_and_missing_modes_migrate_to_blocked() {
        let rule = ApplicationRule::from_window(&window(None, "AnyGame.exe", "AnyClass"));
        assert_eq!(rule.drag_mode, DragMode::Disabled);
        let mut old = serde_json::to_value(&rule).unwrap();
        old.as_object_mut().unwrap().remove("drag_mode");
        let restored: ApplicationRule = serde_json::from_value(old.clone()).unwrap();
        assert_eq!(restored.drag_mode, DragMode::Disabled);
        old["drag_mode"] = serde_json::json!("auto");
        let restored: ApplicationRule = serde_json::from_value(old).unwrap();
        assert_eq!(restored.drag_mode, DragMode::Disabled);
        assert_eq!(
            serde_json::to_value(restored).unwrap()["drag_mode"],
            "disabled"
        );
    }

    #[test]
    fn exact_path_is_case_and_separator_insensitive() {
        let rule = ApplicationRule::from_window(&window(
            Some(r"C:\Games\Example.exe"),
            "Example.exe",
            "GameWindow",
        ));
        assert!(rule.matches(&window(
            Some("c:/games/example.exe"),
            "Example.exe",
            "OtherClass"
        )));
    }

    #[test]
    fn automatic_application_excludes_same_process_splash_class() {
        let rule = ApplicationRule::from_window(&window(
            Some(r"C:\Games\Example.exe"),
            "Example.exe",
            "GameWindow",
        ));
        let splash = window(Some(r"C:\Games\Example.exe"), "Example.exe", "SplashWindow");
        assert!(rule.matches(&splash));
        assert!(!rule.matches_automatic_window(&splash));
        assert!(rule.matches_automatic_window(&window(
            Some("c:/games/example.exe"),
            "Example.exe",
            "GameWindow",
        )));
    }

    #[test]
    fn filename_is_used_when_path_is_unavailable() {
        let mut rule = ApplicationRule::from_window(&window(None, "Example.exe", "GameWindow"));
        rule.window_class_hint = None;
        assert!(rule.matches(&window(None, "EXAMPLE.EXE", "OtherClass")));
    }

    #[test]
    fn class_hint_disambiguates_matching_executable() {
        let rule = ApplicationRule::from_window(&window(None, "Example.exe", "MainWindow"));
        assert!(!rule.matches(&window(None, "Example.exe", "LauncherWindow")));
    }

    #[test]
    fn scan_matching_preserves_path_filename_class_and_rule_order() {
        let windows = [
            window(Some("c:/games/example.exe/"), "Example.exe", "MainWindow"),
            window(Some(r"C:\Other\Example.exe"), "Example.exe", "MainWindow"),
            window(None, "EXAMPLE.EXE", "MainWindow"),
            window(None, "Example.exe", "Splash"),
        ];
        let exact = ApplicationRule::from_window(&window(
            Some(r"C:\Games\Example.exe"),
            "Example.exe",
            "MainWindow",
        ));
        let mut fallback = exact.clone();
        fallback.executable_path = None;
        fallback.window_class_hint = None;
        let rules = [exact, fallback];
        let matches = matching_windows(&rules, &windows);
        for (rule, matched) in rules.iter().zip(&matches) {
            let original: Vec<_> = windows
                .iter()
                .filter(|window| rule.matches_automatic_window(window))
                .collect();
            assert_eq!(matched.len(), original.len());
            assert!(
                matched
                    .iter()
                    .zip(original)
                    .all(|(a, b)| std::ptr::eq(*a, b))
            );
        }
        assert_eq!(matches[0].len(), 2);
        assert_eq!(matches[1].len(), 4);
    }

    #[test]
    fn scan_match_reuse_preserves_large_fixture_results() {
        let windows: Vec<_> = (0..100)
            .map(|index| {
                window(
                    Some(&format!("C:/Games/Fixture{index}/Game.exe")),
                    "Game.exe",
                    "MainWindow",
                )
            })
            .collect();
        let rules: Vec<_> = windows[..50]
            .iter()
            .map(ApplicationRule::from_window)
            .collect();
        let started = std::time::Instant::now();
        let mut original = Vec::new();
        for _ in 0..20 {
            for _ in 0..3 {
                original = rules
                    .iter()
                    .map(|rule| {
                        windows
                            .iter()
                            .filter(|window| rule.matches_automatic_window(window))
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>();
                std::hint::black_box(&original);
            }
        }
        let previous = started.elapsed();
        let started = std::time::Instant::now();
        let mut reused = Vec::new();
        for _ in 0..20 {
            reused = matching_windows(&rules, &windows);
            std::hint::black_box(&reused);
        }
        let current = started.elapsed();
        assert!(original.iter().zip(&reused).all(|(old, new)| {
            old.iter().zip(new).all(|(a, b)| std::ptr::eq(*a, *b)) && old.len() == new.len()
        }));
        println!(
            "rule_matching_fixture: original_three_passes_us={}, reused_once_us={}",
            previous.as_micros(),
            current.as_micros()
        );
    }
}
