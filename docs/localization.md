# Localization

Bald supports Korean, English, Japanese, Simplified Chinese, Traditional Chinese,
German, French, Spanish, Brazilian Portuguese, Italian, Russian, and Turkish.
The brand text `Bald` and `Border Ablation Layout Decoupling` stays in English.

## Language selection

On first launch after installation, Rust reads the ordered Windows user UI
language list with `GetUserPreferredUILanguages(MUI_LANGUAGE_NAME)` and chooses
the first supported language. An empty or unsupported list falls back to English.
The concrete language and its automatic selection status are immediately saved.
While selection is automatic, each Bald process startup checks Windows again and
updates the saved language if needed. Selecting any language in Bald, including
the current language, records a manual choice that survives Windows changes.
Region and keyboard settings are not used. The selector lists only the 12 languages;
there is no system-language option. Windows changes take effect at the next Bald
startup, not while the existing process is running.

Chinese script subtags take precedence over regions. Without an explicit script,
Taiwan, Hong Kong, and Macao map to Traditional Chinese; other Chinese locales
map to Simplified Chinese. Portuguese locales use the Brazilian translation,
which is explicitly labeled `Português (Brasil)` in the selector.

Existing manual selections retain their meaning. Older `system` values and
missing language fields are replaced with a concrete language and automatic
selection status, preserving other settings. Older concrete values without the
selection status are treated as manual choices. Resetting all settings restores
automatic selection with the current Windows language. Both the WebView and tray menu use the saved language;
native diagnostic details remain unchanged below localized error summaries.

## Copy

`web/i18n.js` contains locale names, BCP 47 tags, and complete UI translations.
Use native language names in the selector. Keep product names unchanged, use
short action labels, and distinguish removing a list entry from deleting an
application or personal file. Traditional Chinese uses Taiwanese terminology;
Spanish avoids region-specific pronouns; Portuguese uses Brazilian terminology.

Reference: [Microsoft localization style guides](https://learn.microsoft.com/en-us/globalization/reference/microsoft-style-guides).
Translations have not been reviewed by native speakers. Native review should
cover contextual wording, especially window border removal and dragging controls.

## Fonts

Bundle official v1.3.9 WOFF2 variable fonts locally:

| UI language | Font |
| --- | --- |
| Korean | Pretendard Variable |
| Japanese | Pretendard JP Variable |
| English, European languages, Russian | Pretendard Std Variable |
| Simplified Chinese | Microsoft YaHei UI / Microsoft YaHei |
| Traditional Chinese | Microsoft JhengHei UI / Microsoft JhengHei |

The header always uses Pretendard Std Variable. The three bundled fonts total
7,697,472 bytes, compared with 6,318,784 bytes for the previous four OTF files.
These are asset sizes, not installer size or decoded memory usage. The language
menu is created on first opening so its Japanese and Korean labels do not force
their fonts to load at startup in other UI languages. The selector uses each
language's font for its native name, including on differently localized Windows.

Source URLs and SHA-256 checksums are in `web/fonts/sources.json`; the font license
is packaged in `web/fonts/OFL.txt`. Chinese uses installed fonts with a sans-serif
fallback; font appearance depends on the Windows fonts available. Test Chinese
on Windows without the corresponding language packs before distributing broadly.

## Verification

- `npm run lint`
- `npm test`: complete keys, language changes, brand invariance, accessibility,
  and font checksums, plus existing UI behavior tests.
- `cargo test -- --test-threads=1`: locale mapping, fallback, legacy choices,
  and native behavior. Native resource-count tests can interfere when parallel.
- `cargo build`
- `npm run build:unsigned -- --no-bundle`: Tauri release executable with bundled assets.
- Browser preview with mocked Tauri commands at the 488 × 720 window size:
  language switching, long translations, menus, modal text, and mixed-script titles.
  This preview does not exercise live Tauri IPC or native tray interaction.
