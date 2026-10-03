# Bald

Border Ablation Layout Decoupling is a small Windows 11 tray utility that remembers selected applications
and removes the standard Windows frame whenever their top-level window appears.

## Current milestone

- persistent per-application rules;
- filtered visible-window picker;
- idempotent border removal that preserves geometry;
- session-aware border restoration;
- automatic watcher with a low-frequency reconciliation pass;
- global and per-app enable switches;
- per-user Windows startup registration;
- close-to-tray and single-instance behavior.

## Build

```powershell
cargo build
cargo run
```

서명된 NSIS 설치 파일과 updater artifact를 만들려면 프로젝트 루트에 보관된
`updater.key`가 필요합니다.

```powershell
npm install
npm run build
```

## Release

GitHub 저장소 `mightyidler/Bald`의 Actions secret에 아래 값을 등록합니다.

- `TAURI_SIGNING_PRIVATE_KEY`: 로컬 `updater.key`의 전체 내용
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: 현재 키는 암호가 없으므로 빈 값

변경 사항을 먼저 커밋한 뒤 `npm run release`를 실행하면 patch 버전 증가,
태그 푸시, Windows 설치 파일 배포, `update.json` 갱신이 자동으로 진행됩니다.

Windows 11 is the primary target. Bald is GPL-3.0-only because its core
window-style behavior was derived from the GPL-3.0 `ihateborders` project by
Z1xus: <https://github.com/Z1xus/ihateborders>.
