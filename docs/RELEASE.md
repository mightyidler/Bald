# Bald 릴리즈 확인표

> 사용자가 명시적으로 요청하기 전에는 버전 변경, 태그 생성, 릴리즈를 하지 않는다.

## 릴리즈 전

1. 작업 트리와 원격 동기화 확인: `git status --short`, `git fetch origin main --tags`
2. 버전을 `package.json`, `package-lock.json`, `Cargo.toml`, `Cargo.lock`, `tauri.conf.json`에 동일하게 반영
3. 검증: `cargo fmt --check`, `cargo check --release`, `git diff --check`
4. 변경을 커밋한 뒤 `v<버전>` 태그를 `main`과 함께 push

## 자동 배포 흐름

태그 push → GitHub Actions에서 Windows 설치 파일 빌드·서명 → GitHub Release 게시 → `update.json` 갱신 커밋

## 완료 확인

API를 반복 조회하지 않는다. 잠시 기다린 뒤 아래만 확인한다.

1. `git fetch origin main`
2. `git show origin/main:update.json`의 `version`과 설치 파일 URL 확인
3. `git pull --ff-only origin main`으로 자동 생성된 매니페스트 커밋 동기화
4. 이전 버전 설치본에서 업데이트 후 새 버전 표시와 재실행 확인

## 비밀 정보

- `updater.key`, `.env`, 빌드 디렉터리는 커밋 금지
- `updater.key.pub`와 `update.json`의 서명은 공개 가능
- 릴리즈 전 `git status --short --ignored`와 추적 파일의 토큰·개인키 패턴을 점검
