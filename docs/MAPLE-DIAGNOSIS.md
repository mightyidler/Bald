# MapleStory 창 문제: 현재 결과

2026-10-09. 버전0.1.2 유지. 릴리즈/tag/push 없음.

## 확인한 원인 조건

- 실제 Maple PID11244/HWND1006b2, 창모드1920×1080, DPI168(175%).
  원본 style14ce0000/ex100, outer1944×1144, 내용 오프셋12,52.
- 원래 상단바 합성 드래그는 게임 캡처 상태에서 Y540→684, 정상 이동144px.
- WS_CAPTION만 제거한 style140e0000/ex100에서도 내용 상단25px를 잡으면
  게임이 캡처하고 Y581→0으로 이동한다. 전체 제거/비동기 갱신에서도 Y592→0.
- THICKFRAME만 제거하면 같은 내용 위치에서 게임 캡처/창 이동 없음.
  EX_WINDOWEDGE 단독 시험은 실제 read-back에서100으로 돌아왔으므로
  독립 조건으로 유지된 시험이 아니다. 그 결과로 원인 비트를 단정하지 않는다.
- 따라서 CAPTION 제거만으로 문제를 유발함은 확인. 게임 내부의 정확한 함수나
  계산식까지 확인한 것은 아니다. 같은 기초 입력 대조를 다시 반복하지 않는다.

## 반영한 수정

- MapleStoryClass만 원래 style/exstyle과 논리적 캡션을 유지한다.
- DWM 비클라이언트 그림을 끈 뒤 실제 내용 영역으로 창 region을 제한한다.
  기존 실패 후보는 DWM 그림을 그대로 둔 region 방식이었다. 그것을 재사용하지 않는다.
- 프레임 그림 정책의 재계산을 먼저 완료한 뒤 region을 설치한다.
  내용 좌표/크기가 달라졌을 때만 소유 스레드의 비동기 재계산을 추가한다.
- 복구는 원래 region과 DWM 렌더링 상태를 돌린다. 실패하면 소유 상태를 유지한다.
  전체 창 사각형과 동일한 native region은 별도 사용자 crop으로 다시 설치하지 않는다.
  실제 사용자 custom region/원래 classic rendering은 별도 회귀 검사로 보존 확인.
- 드래그는 보이는 내용 맨 위16px만 Bald가 처리한다. 그 아래 게임 UI 입력은 전달한다.
  게임이 잘못된 자체 캡션 이동을 시작하지 않아 아래쪽 입력 차단을 넓힐 필요가 없다.
- 다른 게임의 기존 스타일 제거 경로, Bald UI 표시 상태, 버전은 변경하지 않는다.
  제품 경로에서 게임 숨김/최소화/강제 표시/합성 입력/ClipCursor 변경 없음.

## 실제 입력 검증

사용자가 자동 드래그 시험을 명시적으로 허용했다. 테스트 프로세스만 SendInput을
사용하며 동일 HWND/PID와1시간 이내 승인 기록이 필요하다. 게임 코드 주입 없음.
테스트는 앱의 public WindowController와 실제 Watcher 마우스 훅을 사용했다.

| 검증 항목 | 실제 결과 |
| --- | --- |
| 내용 상단5px 허용 이동 / 화면 최상단 | 아래144px 이동 / clientY=0, 위쪽 빈 공간 없음 |
| 아래25px / 차단5px·25px / 중앙 배치 | 캡처·이동 없음 / 이동 없음 / 내용 원점960,540 |
| 최소화 / 다시 표시 | visible=true·iconic=true / 정상 표시·내용1920×1080 유지 |
| 복구 / 중복 적용·반복3회 | 최신 상단바 캡처 확인 / 원점932,592·1920×1080·nativeRendering=true |

- 최종 앱 경로 기록: artifacts/bald-test-session/trial-11.txt, exit0.
- 화면 근거: caption-before.png / caption-clipped-disabled.png / caption-after.png.
  게임 창 DC가 아니라 실제 데스크톱 합성 화면의 상단100px을 캡처했다.
- 검사 준비 오류도 보존: trial-2 활성화 거절, trial-4 시험 프로세스 worker DPI 불일치,
  trial-6 SW_SHOWNOACTIVATE가 실제 게임 최소화를 풀지 못함. 최종 시험은
  프로세스 DPI V2 및 소유 스레드 SW_RESTORE로 진행해 통과했다.
- 정상 Rust 검사50통과/0실패/수동4제외. 일반 개발 빌드 및 fmt 검사 exit0.
- 아직 미검증: 게임 설정 UI의 해상도 변경, Win+방향키, 장시간 반복,
  다른 실제 게임2종, Bald 전체 UI의 OFF/삭제/종료 버튼을 통한 E2E.
  위 범위까지 모두 해결됐다고 확대하지 않는다.
- 테스트 뒤 메이플은 원래 상단바/1920×1080/위치920,540으로 복구. Bald는 미실행.
  새 개발 빌드는 target/debug/bald.exe. --window-diagnostics 사용 금지.

## 코드 및 참고

- src/window_manager.rs: make_borderless → clip_native_frame, native region 복구.
- src/window_manager_windows_tests.rs: native/classic/custom-region/중복·복구 회귀 검사.
- src/maple_style_trials.rs: 승인된 실게임 스타일 분리/실제 훅/복구 시험, cfg(test) 전용.
- 로컬 scripts/artifacts는 Git ignore. 관리자 검사 세션PID16932는8시간 한정이며
  기존 세션 재사용으로 추가 UAC 없이 이번 자동 시험들을 실행했다.
- [DWM 그림 정책](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwmncrenderingpolicy),
  [SendInput](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput).
