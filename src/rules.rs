use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::window_manager::WindowInfo;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationRule {
    pub id: Uuid,
    pub display_name: String,
    pub executable_name: String,
    pub executable_path: Option<String>,
    pub window_class_hint: Option<String>,
    pub enabled: bool,
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
        }
    }

    pub fn matches(&self, window: &WindowInfo) -> bool {
        if let (Some(expected), Some(actual)) = (&self.executable_path, &window.executable_path) {
            return normalize_path(expected) == normalize_path(actual)
                && self.class_matches_when_needed(window);
        }
        self.executable_name
            .eq_ignore_ascii_case(&window.executable_name)
            && self.class_matches_when_needed(window)
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
    path.replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(path: Option<&str>, name: &str, class: &str) -> WindowInfo {
        WindowInfo {
            hwnd: 1,
            title: "Example".into(),
            executable_name: name.into(),
            executable_path: path.map(str::to_owned),
            class_name: class.into(),
            is_borderless: false,
        }
    }

    #[test]
    fn exact_path_is_case_and_separator_insensitive() {
        let mut rule = ApplicationRule::from_window(&window(
            Some(r"C:\Games\Example.exe"),
            "Example.exe",
            "GameWindow",
        ));
        rule.window_class_hint = None;
        assert!(rule.matches(&window(
            Some("c:/games/example.exe"),
            "Example.exe",
            "OtherClass"
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
}
