use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use windows::{
    Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME},
    core::PWSTR,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    #[serde(rename = "system")]
    // Only a migration marker for older or missing language settings.
    LegacySystem,
    Korean,
    English,
    Japanese,
    SimplifiedChinese,
    TraditionalChinese,
    German,
    French,
    Spanish,
    PortugueseBrazil,
    Italian,
    Russian,
    Turkish,
}

impl Language {
    pub fn detect() -> Self {
        static SYSTEM_LANGUAGE: OnceLock<Language> = OnceLock::new();
        *SYSTEM_LANGUAGE.get_or_init(|| preferred_language(&windows_ui_languages()))
    }

    pub fn tray_labels(self) -> (&'static str, &'static str) {
        match self {
            Self::Korean => ("열기", "종료"),
            Self::Japanese => ("開く", "終了"),
            Self::SimplifiedChinese => ("打开", "退出"),
            Self::TraditionalChinese => ("開啟", "結束"),
            Self::German => ("Öffnen", "Beenden"),
            Self::French => ("Ouvrir", "Quitter"),
            Self::Spanish => ("Abrir", "Salir"),
            Self::PortugueseBrazil => ("Abrir", "Sair"),
            Self::Italian => ("Apri", "Esci"),
            Self::Russian => ("Открыть", "Выход"),
            Self::Turkish => ("Aç", "Çıkış"),
            Self::LegacySystem | Self::English => ("Open", "Exit"),
        }
    }
}

fn language_for_locale(locale: &str) -> Option<Language> {
    let locale = locale.replace('_', "-").to_ascii_lowercase();
    let mut parts = locale.split('-');
    let base = parts.next()?;
    Some(match base {
        "ko" => Language::Korean,
        "en" => Language::English,
        "ja" => Language::Japanese,
        "zh" => {
            let subtags: Vec<_> = parts.collect();
            // An explicit script takes priority over the region (e.g. zh-Hans-HK).
            if subtags.contains(&"hans") {
                Language::SimplifiedChinese
            } else if subtags.contains(&"hant")
                || subtags
                    .iter()
                    .any(|part| matches!(*part, "tw" | "hk" | "mo" | "cht"))
            {
                Language::TraditionalChinese
            } else {
                Language::SimplifiedChinese
            }
        }
        "de" => Language::German,
        "fr" => Language::French,
        "es" => Language::Spanish,
        "pt" => Language::PortugueseBrazil,
        "it" => Language::Italian,
        "ru" => Language::Russian,
        "tr" => Language::Turkish,
        _ => return None,
    })
}

fn preferred_language(locales: &[String]) -> Language {
    locales
        .iter()
        .find_map(|locale| language_for_locale(locale))
        .unwrap_or(Language::English)
}

fn windows_ui_languages() -> Vec<String> {
    let mut count = 0;
    let mut length = 0;
    // Query the UTF-16 buffer size before letting Windows write into it.
    unsafe {
        if GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, None, &mut length).is_err()
            || length == 0
        {
            return Vec::new();
        }
        let mut buffer = vec![0u16; length as usize];
        if GetUserPreferredUILanguages(
            MUI_LANGUAGE_NAME,
            &mut count,
            Some(PWSTR(buffer.as_mut_ptr())),
            &mut length,
        )
        .is_err()
        {
            return Vec::new();
        }
        buffer
            .split(|value| *value == 0)
            .filter(|part| !part.is_empty())
            .map(String::from_utf16_lossy)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_regional_locales_and_chinese_scripts() {
        for (locale, expected) in [
            ("ko-KR", Language::Korean),
            ("en-GB", Language::English),
            ("ja-JP", Language::Japanese),
            ("zh-CN", Language::SimplifiedChinese),
            ("zh-SG", Language::SimplifiedChinese),
            ("zh-TW", Language::TraditionalChinese),
            ("zh-HK", Language::TraditionalChinese),
            ("zh-MO", Language::TraditionalChinese),
            ("zh-Hant-CN", Language::TraditionalChinese),
            ("zh-Hans-HK", Language::SimplifiedChinese),
            ("de-AT", Language::German),
            ("fr-CA", Language::French),
            ("es-MX", Language::Spanish),
            ("pt-BR", Language::PortugueseBrazil),
            ("pt-PT", Language::PortugueseBrazil),
            ("it-IT", Language::Italian),
            ("ru-RU", Language::Russian),
            ("TR_tr", Language::Turkish),
        ] {
            assert_eq!(language_for_locale(locale), Some(expected), "{locale}");
        }
    }

    #[test]
    fn uses_first_supported_preference_and_falls_back_to_english() {
        assert_eq!(
            preferred_language(&["th-TH".into(), "ja-JP".into(), "ko-KR".into()]),
            Language::Japanese
        );
        assert_eq!(preferred_language(&["ar-SA".into()]), Language::English);
        assert_eq!(preferred_language(&[]), Language::English);
    }

    #[test]
    fn saved_manual_choices_remain_compatible() {
        for (json, expected) in [
            ("\"korean\"", Language::Korean),
            ("\"english\"", Language::English),
        ] {
            let language: Language = serde_json::from_str(json).unwrap();
            assert_eq!(language, expected);
            assert_eq!(serde_json::to_string(&language).unwrap(), json);
        }
        assert_ne!(Language::detect(), Language::LegacySystem);
    }
}
