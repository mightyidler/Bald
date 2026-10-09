# Bald — 현재 수정·검증 상태

2026-10-09. 버전 **0.1.2 유지**, 사용자 요청 전 버전 변경·릴리즈·push 금지.

## 초기화 모달 하단 잔상

- 영상 `20261009-0834-35.9066969.mp4` 5초에서 확인: 문제 네모는 맨 아래 가로 스크롤바. 버튼 배경/숨겨진 추가 패널이 아니다.
- 원인 재현: main 블러가 fixed 드롭다운의 containing block을 바꿔 가로 넘침 발생. overflow-y:auto로 인해 지정하지 않은 overflow-x도 auto가 됨. 미리보기 clientWidth470/scrollWidth531, offsetHeight709/clientHeight694(가로 막대15px).
- 수정: main·picker-list에 overflow-x:hidden. 동일한 legacy main 블러 조건으로 다시 확인: scrollWidth531이어도 clientHeight=offsetHeight709, 가로 막대 없음. 세로 스크롤/하단16px 유지. Node17개·cargo check 통과. 실행 중 bald.exe 때문에 개발 실행 파일 갱신은 아직 필요.
- 후속 변경: `main`/header의 filter 합성을 제거하고 dim 레이어의 backdrop-filter(8px)로 배경 블러를 옮겼다. 미리보기에서 열기/취소와 main filter=none 확인. 실제 WebView 잔상 해결은 미확인.
- 488×720 UI 미리보기에서 초기화 버튼/취소 포커스로 `.app` 자체가 약 259px 스크롤되어 숨겨진 추가 패널이 하단에 노출되는 현상 재현.
- 앱 루트는 `overflow:clip`, 초기화 포커스는 `preventScroll:true`. 닫힌 추가 패널·드롭다운·초기화 모달은 퇴장 모션 이후 `visibility:hidden`으로 렌더링/포커스 대상에서 제외. 기존 모션 시간은 유지.
- 반복 초기화→취소, 드롭다운, 추가 패널 열기→닫기 미리보기 확인. 루트 scrollTop=0, 닫힌 표면 visibility=hidden. Node15개·개발 빌드 통과, 기존 lint 줄 간격 오류는 실패 유지. 실제 Bald WebView2 화면은 별도 미검증.

## 관리자 승인 뒤 소켓 10035

- 실제 Windows 테스트에서 동일 메시지 재현: 비동기 소켓 작업을 즉시 완료하지 못했습니다(os error 10035). Winsock accept가 비동기 listener 모드를 상속하는데 read_exact/write_all 프로토콜에 blocking 전환이 누락됨.
- accept 직후 handshake 전에 set_nonblocking(false) 적용. 기존 timeout·인증 토큰은 유지.
- 실패하던 지연 handshake 테스트가 수정 후 통과. 비동기 listener를 쓰는 실제 자식 작업자 테스트(OFF/연결 종료 복구·UI 상태 유지) 통과. 서비스 단위 3개·Node16개·fmt 통과. 개발 빌드는 실행 중 bald.exe 잠금(os error 5)으로 실패, 실제 UAC 조합은 재검증 전.

## 관리자 작업 중 Bald 창 유지

- 권한 전환에 `restart_as_admin`/UI 종료를 사용하지 않는다. UI PID/HWND를 유지하고 `--border-worker` 관리자 작업 프로세스만 시작한다. 같은 실행 세션에서는 재사용한다.
- 작업 프로세스는 UI 없이 기존 WindowController·Watcher로 적용/복구/이동을 처리한다. UI PID는 대상 열거에서 제외한다. 초기 로컬 작업자는 복구·정지 후 권한 작업자에게 넘긴다.
- loopback 전용 연결 + 실행별 토큰 + 1MiB 메시지 제한. 명령은 설정 동기화·상태 조회·등록 창 복구·창 이동·종료만 허용하며 외부 실행/파일 쓰기 명령은 없다.
- UAC 취소/실행 실패 전에는 로컬 작업자를 유지한다. UI 연결이 끊어지면 작업자가 종료되며 관리 중인 창을 복구한다. UI 명령은 백그라운드에서 처리한다.
- 별도 native 시험 창으로 표시/최소화/숨김 각각 위치·상태·z-order 유지, 적용 3회, OFF 복구, 연결 종료 복구 통과. 실제 게임·기존 Bald는 조작하지 않았다.
- 개발 빌드·Node13통과. Rust52통과 실행 후 최종 전체 재검사는51통과/1실패/수동6제외: 기존 `native_region_preserves_styles_content_and_restores_each_cycle`의 native rendering 확인이 간헐 실패했고 단독 재검사는 통과했다. 실제 UAC 승인/취소 Bald UI E2E는 미검증. 기존 lint 공백 오류는 유지.
- 설치판의 기존 관리자 예약 작업은 다음 실행용으로 유지한다. 현재 UI가 이미 열린 뒤 권한이 필요하면 작업 프로세스만 승인받아 시작한다. 개발 실행은 세션마다 필요할 때 한 번 승인한다.

## 관리자 재시작 후 닫기·최소화

- 원인: 초기 `visible:false`인 창을 native snapshot으로 표시하면 Tao의 VISIBLE 캐시는 false로 남는다. 첫 `hide()`는 변경 없음으로 무시되고, `minimize()`가 숨김 상태를 적용한다.
- 수정: 사용자 닫기·최소화 시 실제로 표시 중인 창만 Tauri `show()`로 캐시를 맞춘 뒤 각각 `hide()`·`minimize()` 처리. Alt+F4도 동일 닫기 경로. 자동 재시작·위치·포커스 보존 경로는 변경하지 않는다.
- 같은 Tao 0.37.1 시험 창에서 기존 증상 재현, 수정 경로 3회 반복 통과 (`artifacts/ui-visibility-repro.rs`). Rust50통과/수동4제외, Node12통과, cargo check/fmt 성공.
- 실제 관리자 Bald의 버튼 입력 E2E는 아직 미검증. 실행 중인 Bald·게임을 종료하거나 조작하지 않았다.

## 현재 창 이동 설정

- 최신 근거는 [MAPLE-DIAGNOSIS.md](MAPLE-DIAGNOSIS.md)만 우선 읽는다.
- 실제 자동 입력 분리 시험에서 WS_CAPTION 제거만으로 게임 캡처/Y=0 이동 재현.
  비동기 갱신만 바꾼 후보는 이 문제를 해결하지 못했다.
- MapleStoryClass는 논리적 캡션/style/ex 유지 + DWM 프레임 그림 비활성화 +
  client region으로 변경했다. 이전 DWM 보존 region 후보와 구분할 것.
- 앱의 public 적용/복구 코드와 실제 Watcher 훅을 사용한 실게임 시험 통과:
  상단5px 아래144px 이동, clientY=0 도달, 아래25px 이동 없음,
  차단5px·25px 이동 없음, 내용 기준 중앙 배치, 최소화 중 visible=true,
  정상 복원, 최신 상단바 복구, 중복/반복3회 내용1920×1080 유지.
- Rust50통과/0실패/수동4제외, 일반 개발 빌드/format 검사 성공. 버전0.1.2 유지.
- Win+방향키·게임 해상도 UI 변경·장시간 반복·다른 실게임2종·전체 Bald UI E2E는
  아직 미검증. 해결 범위를 확대하지 않는다. 시험 뒤 게임 원래 상태 복구, Bald 미실행.
- 관리자 검사 세션PID16932는8시간 한정으로 재사용. 이번 자동 시험마다 UAC 재요청 없음.

아래는 이전 작업 이력이며 최신 구현/검증 상태를 덮어쓰지 않는다.

## 이전 작업 기록: 공통 드래그 제한 (자동 판별은 폐기)

- 재실행 후 옵션 미적용 수정: 실제 Valorant PID24052/HWND509d6 읽기에서 marker14cf0000·style140b0000·여백0 확인, 설정은 enabled 저장됨. 이미 borderless면 make_borderless가 원본 snapshot 등록 전에 반환하는데, 드래그는 snapshot 존재를 요구했던 모순. 현재 세션의 복구 소유권과 등록 창 드래그 자격 분리. 허용된 HWND/PID·현재 borderless·표시/비최소화·복구 중 아님을 검증해 이동. 원본 기록 없는 창은 명시적 허용 시 상단32px 사용, 스타일/크기/DWM 복구값 추정·소유권 채택 없음. 기본 Auto/차단은 그대로 클릭 통과. 고정 게임 위치 제약 및100ms 중단 유지.
- 재시작 모사 시험: 이전 controller로 테두리 제거→새 controller에서 mode 변경. 이전 코드 Enabled에서도 hook 대상 없음으로 실패→수정 후 Enabled 이동/Disabled 중지/복구 소유 상태 비어 있음 통과. 최종 Rust43 통과/0 실패/수동2 제외, Node7 통과, fmt/diff 공백 검사 통과. 실제 Valorant 입력·이동 시험은 수행하지 않음. 읽기 스크립트 `scripts/read-drag-state.ps1` ignore 확인. 기존 Bald PID26824 실행 중이라 정상 target/debug exe는 아직 갱신 전, 별도 target-verify 빌드 성공. 앱 강제 종료·자동 실행 없음.

- 후속 응답 없음 수정: 기존 동기 `set_application_drag_mode`가 UI에서 config write/관리 창 조회를 대기. scan은 config read를 가진 채 같은 프로세스 창에 GetWindowText(WM_GETTEXT)를 요청할 수 있어 UI↔scan 대기 고리가 생김. 자기 PID를 제목 조회 전에 제외하고, 설정 저장과 get_state를 blocking worker로 이동. 드래그 모드 저장은 창 메타데이터 조회 없이 config 변경→저장→watcher 알림으로 처리.
- 자기 창 제외 시험: 수정 전 WM_GETTEXT 2회로 실패→수정 후 0회 통과. 최종 전체 Rust 42 통과/0 실패/수동 2 제외, 정상 빌드 갱신, Node 7 통과. 실제 WebView에서 직접 허용 선택은 아직 사용자 재확인 필요. 최초 전체 검사에서 구 영역 시험 1회 실패, 단독·전체 재검사 통과. 해당 구 시험 함수는 실제 GetAsyncKeyState(VK_LBUTTON)에 따라 적용을 건너뛰므로 사용자 클릭에 의존하는 기존 한계가 있음. 제품 코드를 바꿔 시험을 억지로 통과시키지 않음.

- 게임명 예외 없음. 목록의 창 이동 설정: `이동 확인 후 허용`(기본) / `직접 허용` / `허용 안 함`. 기존 설정도 기본값 적용.
- 기본값은 미확인 창 상단 클릭을 가로채지 않음. 등록된 창의 원래 프레임에서 Move 메뉴 활성·HTCAPTION 확인 후, 실제 native 이동 시작→종료에서 원점 변경·크기 유지가 관측된 HWND/PID만 허용. 확인하려면 자동 제거 OFF에서 원래 상단바로 이동해야 하며, 실제 프레임 복구가 안 된 창은 이 방법으로 확인 불가. 가운데 위치·실행파일 이름만으로 판정하지 않음. 창이 새로 생기면 다시 미확인.
- 이미 활성화된 창만 사용자 드래그 시작 가능. 마우스 훅에서 게임 hit-test 메시지·대기 잠금 제거. 이동 좌표는 최신 1개로 병합, 보간/완화 모션 없음.
- 위치 요청은 한 번에 1개. 실제 원점 반영 전 추가 요청 없음, 100ms 미반영이면 그 창 드래그 중단. 대기 중에만 16ms 재확인하며 평소 polling 없음. 재허용은 설정 모드 변경 필요. 사용자 드래그에 NOSENDCHANGING을 제외해 owner 위치 제한을 무시하지 않음. 적용·복구 위치 요청의 기존 플래그는 유지.
- 이미 Windows에 전달된 비동기 요청 1개는 취소할 수 없음. 실제 게임 렌더링 지연이나 OFF 직전 요청과 복구의 순서는 미검증. 메이플 즉시 상단바 복구 문제는 별도 미해결.
- 검증: Rust 41 통과/0 실패/수동 2 제외, Node 7 통과, fmt·JS 구문·git diff 공백 검사 통과. 미확인 허용/단일 요청 제한을 제거하면 새 시험 실패→복원 후 통과 확인. 모사 Win32 창 시험이며 실제 Valorant 이동 시험이 아님.
- 정상 `cargo build --bin bald` 성공, `target/debug/bald.exe` 갱신. 사용자 정상 종료 뒤 빌드했고 자동 실행·게임 조작 없음. `npm run lint` 기존 간격 오류 128개 잔존, 이번 새 드래그 UI 구간에는 지적 없음. 규칙 완화 없음.

## 실행 제약

- 사용자 직접 `cargo run`. `--window-diagnostics` 사용 중단.
- 앱 자동 실행, 실게임 자동 입력·SC_MOVE·SC_RESTORE·숨김·최소화·종료 금지.
- 사용자 CSS/AGENTS/스킬/린터 변경 보존. 린트 규칙 완화 금지.

## 현재 구현

- 메이플 전용 영역 자르기 신규 경로 제거. 모든 창에 일반 프레임 + 확장 프레임 4종 제거/복구 사용. 내용 크기·화면 원점 유지.
- 적용·복구 위치 변경에 NOSENDCHANGING/NOACTIVATE/NOZORDER/NOOWNERZORDER 적용. 사용자 드래그는 owner 위치 제한을 존중하도록 NOSENDCHANGING 제외. 이것이 실제 메이플 점프 원인이라는 결론은 미검증.
- 후속 반복 복구 수정: 스타일 복원→NOMOVE/NOSIZE 프레임 갱신 알림→실제 여백 측정→내용 크기 유지 순서. 복구 알림에는 NOSENDCHANGING을 사용하지 않고, 실제 이동/크기 변경에는 유지.
- 지연 복구 확인: 일반 복구 알림 후 최대5초, 100ms 간격으로 실제 상단 여백/DWM 상태 읽기만 수행. 반복 스타일·프레임 갱신 없음, 평소 polling 없음. OFF/삭제/초기화는 worker에서 실행해 UI 대기 방지. 대기 중 사용자가 이동한 창은 최신 원점 유지. 시간 초과는 소유 상태를 보존하고 오류 보고. 복구 지연 자체를 없앤 수정은 아님.
- 적용 전 DWM 프레임 렌더링 상태 저장. 원래 켜져 있었는데 복구 시 꺼져 있으면 활성화 및 재확인. 원래 클래식 창에는 강제 적용하지 않음. 이전 코드가 이미 클래식 상태로 만든 창의 과거 렌더링 상태는 추정하지 않음.
- 드래그: 최초 창 원점 + 최초 클릭 이후 마우스 이동량. 40ms 대기·SC_MOVE 제거. 허용된 창 상단 클릭/해제만 소비, 마우스 이동 자체 통과. worker에서 좌표 변경, 이동 이벤트 병합. 정책·시간 제한은 위 최신 작업 참조.
- 드래그 위치 요청은 ASYNCWINDOWPOS 사용. 실제 다른 프로세스에서 OFF 직전 대기 중 위치 요청과 복구의 순서·결과는 추가 확인 필요.
- OFF/삭제/추가 취소/종료: 수정한 HWND 직접 복구, 다른 PID·재사용 HWND 제외. 복구 실패는 오류 표시 및 상태 유지. 실제 상단 여백 확인 없이 성공 처리 금지.
- 구버전 ClippedFrame 마커: 게임 자체 창모드 재설정으로 실제 프레임이 복구된 경우만 정리. 이전 스타일 방식의 미복구 caption 마커도 새 정상값으로 저장하지 않음.
- Bald 자체 프레임: 자기 UI 소유 스레드에만 WM_NCCALCSIZE 두 형식을 처리하는 subclass 설치. 타 프로세스에 설치하지 않음.
- 관리자/업데이트 재실행: UI 표시·숨김·최소화·위치·앞선 창 HWND/PID를 인자로 전달. NOACTIVATE로 앞뒤 순서 복원, 앞선 창이 사라졌으면 맨 뒤로 배치. 예약 작업 우회로 인자 유실 방지. UAC 취소에도 기존 상태 재적용. 실제 UAC 동작은 사용자 확인 필요.
- X/최소화: 헤더의 중복 Tauri 드래그 속성 제거, 버튼 pointer capture 및 버블링 차단. 모션 종료 또는 fallback 후 1회 실행, hide 오류 표시. 기존 모션 토큰/감소 모션 유지.
- 감지: Windows 이벤트 + 4초 예비 타이머, 중복 병합. 관리 창 없거나 자동 OFF면 마우스 훅 해제.
- 아이콘: 첫 EXE 아이콘 그룹의 PNG/DIB 직접 해독, 실제 그림 픽셀이 많은 후보 선택. Shell은 실패 시 fallback, EXE 실행 없음.

소스 비교·근거: [BORDERLESS-RESEARCH.md](BORDERLESS-RESEARCH.md). 구 영역 함수는 시험 코드와 구버전 복구에만 남음.

## 확인된 과거 실패, 반복하지 말 것

- 실제 Maple PID25976/HWND600afc에서 영역 제거 후 스타일14CE0000이 돌아와도 내용=외곽1920×1080, 상단 여백0. OFF뿐 아니라 Bald 종료도 실패한 기록 있음.
- caption 토글, 원래 외곽 크기 요청, WM_THEMECHANGED, SetWindowRgn(NULL,TRUE), FRAMECHANGED(NOREDRAW 제외) 실제 제한 검사 모두 프레임 미복구. API 성공은 복구 성공이 아님.
- 사용자 직접 해상도 변경→1920×1080 재적용 후 같은 HWND/스타일/영역 없음에서 외곽1944×1144, 내용1920×1080, 여백12/52/12/12 복구. 게임 내부 원인은 미확정.
- 이 결과 때문에 영역 자르기 방식을 폐기. 모사 창 통과를 실제 메이플 해결 증거로 사용 금지.

## 이전 복구 작업 검증 기록

- 2026-10-09: 현재 Bald/Maple 미실행 확인. 복구 확인 중 최소화된 창을 실제 caption 복구 성공으로 처리하는 오류 수정. 복구 시작부터 최소화돼 있던 창의 기존 복원 경로는 유지. 실패 상태·마커 보존, 게임 자동 조작 없음. 다음 후보는 실제 크기 변경에 NOSENDCHANGING을 제외해 owner 알림 전달 여부를 비교하는 1회 제한 검사. 아직 실제 검사/제품 반영 안 함, 별도 사용자 허가와 실패 상태 준비 필요.
- `cargo fmt -- --check`: 통과.
- `cargo test --bin bald -- --test-threads=1`: 37 통과, 0 실패, 수동 읽기 검사 2개 제외.
- `cargo build --bin bald`: 2026-10-09 성공, 정상 `target/debug/bald.exe` 갱신. 자동 실행 없음. 버전0.1.2 유지.
- 최소화 판정 회귀 시험: 기존 확인 함수가 actual caption 없는 최소화 창에 Ok(true) 반환해 실패→수정 후 미확인 오류·관리 상태 유지 검증 통과. 처음부터 최소화된 창 복원 시험도 통과. 실제 Maple 즉시 복구를 해결했다는 의미 아님.
- Y=0 강제 위치 보정 시험 창: NOSENDCHANGING 제거 시 실패→복원 시 통과 확인.
- 화면 밖 시험 창: 프레임/확장 스타일 왕복, 다른 플래그 보존, 고정 기준 이동·Y=0 도달·크기 유지, 복구 후 이동 중지, 복구 거부/재시도 확인. 실제 게임과 별개.
- 반복 복구 알림 시험: 이전 경로 실패→수정 후 ON/OFF 10회 통과. DWM 복원 비활성화 시 렌더링 검사 실패→활성화 후 통과. 실제 메이플과 별개.
- 별도 소유 스레드가 200ms 뒤 프레임을 복구하는 시험: 기존 코드 동일 오류로 실패→재확인 수정 후 3회 연속 통과. 스타일 재적용 없음·내용 크기 보존 확인. target-verify 최신 빌드 성공, 정상 target/debug는 실행 중 PID24220으로 아직 미갱신. 자동 실행 없음.
- Bald UI Win32 시험: 숨김·최소화·뒤쪽 순서·위치 유지, 포커스 변화 없음. 실제 UAC 팝업 시험은 아님.
- `node --test tests/window-actions.test.cjs`: 4 통과. X fallback/중복 실행 방지/감소 모션/오류 표시/입력 캡처 시험. 실제 WebView 클릭은 별도 확인.
- `npm run lint`: 기존 간격 규칙 오류 133개. 이번 새 버튼 처리 줄에는 지적 없음. 미해결, 규칙 유지.
- strict Clippy: 기존 코드·시험 코드 지적 잔존. 성공 처리하지 않음.

## 실제 메이플에서 남은 확인

사용자 후속 보고: 새 스타일 방식도 첫 복구 후 반복 실패, 성공 시 클래식 상단바. 해결 완료 아님. 읽기 확인 PID4704/HWND80bf4: 현재 ON 상태는 스타일140A0000·내용1920×1080·여백0·DWM NC 꺼짐·영역 자르기 마커 없음. 이 값만으로 OFF 실패 원인 확정 금지.

같은 실제 HWND의 사용자 OFF 후: 스타일14CE0000·확장100 복원, 내용=외곽1920×1080·상단 여백0·DWM NC 꺼짐·OriginalStyle 유지. 스타일 복원만으로 부족함 확인. 이전 Bald 정상 종료 후 게임은 최소화 상태이고 마커 제거됨, 프레임 복구 성공은 확인 불가. 새 빌드로 실행/창 조작은 하지 않음.

최신 실제 실패: 새 메이플 PID29728/HWND40e22, 사용자 이미지 오류 `native caption restoration incomplete`. 뒤이은 읽기에서 최소화 상태·스타일34CE0000·DWM NC 켜짐·복구 마커 유지. DWM 활성화만으로 해결 불가. 원래 게임은 최신 상단바라는 사용자 확인이 있으며 초기 게임 상태 탓으로 돌리지 말 것.

같은 HWND40e22 후속 읽기: 최소화 해제 상태, 외곽1944×1144·내용1920×1080·여백12/52/12/12·DWM 켜짐. 복구 마커는 여전히 남음. 게임 창 변경 없이 읽기만 수행. 오류 시점과 현재 시점의 실제 프레임 상태가 다르며, 그 사이 사용자 동작·복구 지연·현대/레거시 외관은 미확인. 즉시 실패 판정 이후 프레임이 생겼을 가능성을 조사하되 원인 확정 금지.

사용자 후속 확인: 현재는 최신 Windows 상단바이며 시간이 지나 복구되는 것으로 보임. 원래 형태 복귀는 확인됐지만 실제 지연 시간·유발 이벤트는 미측정. 새 5초 재확인 경로는 실제 게임에서 아직 미검증. 일반 실행 작업 기록으로 다음 ON/OFF에서 대기 시작/확인/시간 초과를 구분할 것.

최신 사용자 재현으로 위 시간 경과 가설 정정: OFF만으로는 복구되지 않고 최소화→다시 표시 때 정상 최신 상단바 복구. 실제 PID30408 기록/HWNDb40c1a: 스타일14ce0000·DWM true가 돌아와도 상단 여백0으로 5초 시간 초과 두 차례. 세 번째는 대기 중 minimized=true를 성공 조건으로 받아 `restore_complete` 기록, 실제 보이는 caption 복구 증거가 아님. 후속 읽기에서는 외곽1944×1144·내용1920×1080·상단52 확인. 5초 대기는 실제 문제 해결이 아니며 대기 시간 증대로 해결하려 하지 말 것.

다음 제한 검사 후보: 새 일반 스타일 경로에서 NOMOVE/NOACTIVATE/NOZORDER를 유지한 실제 외곽 크기 변경이 프레임을 갱신하는지 확인. 현재 코드는 여백0이면 실제 크기 보정 전에 중단한다. 최소화 과정의 어떤 이벤트가 원인인지는 미확정. 구 region 기반 실패와 혼동 금지. 실제 게임 쓰기 검사는 사용자 허가 후 1회만, 실패 시 기존 크기 복귀. 자동 최소화/표시 토글 해결책 금지.

위 크기 검사 실행 완료, 실패: 사용자 허가 후 Maple PID21152/HWNDb40c1a에 1회 크기 변경+실패 롤백만 실행. DPI168·AdjustWindowRectExForDpi 계산1944×1144. SetWindowPos NOMOVE/NOACTIVATE/NOZORDER/NOOWNERZORDER/NOSENDCHANGING/FRAMECHANGED 성공했지만 즉시/250ms/1초 모두 외곽=내용1944×1144, 상단 여백0·DWM true 유지. 기존1920×1080으로 롤백 성공, 위치958,683·표시·최소화 상태 변화 없음, probe 전후 foreground66942 동일. 진단 helper만 숨김 관리자 실행, 게임 입력/최소화/숨김/이동/종료 없음. 기록 `artifacts/size-only-probe-21152.json`, 스크립트 `scripts/probe-size-only-once.ps1` 모두 ignore. 성공 API의 LastError203은 잔여 값이라 실패 근거로 쓰지 말 것. 외곽 크기 변경만으로 복구한다는 가설 기각, 이 방식을 제품 코드에 넣지 않음. 추가 실제 쓰기 검사는 새 허가 필요.

최소화 비교 완료: 동일 PID/HWND를 process-filtered OUTOFCONTEXT WinEvent로 읽기만 기록, 사용자 직접 최소화/복원. `artifacts/minimize-compare-21152.json` 12표본, `scripts/compare-minimize-frame-readonly.ps1` 모두 ignore. baseline 및20.019초 FOREGROUND/20.033초 FOCUS 시 스타일14ce0000·확장100·DWM1·여백0·외곽=내용1920×1080. 20.571초 MINIMIZESTART는 style34ce0000/내용0. 21.635초 복원 LOCATIONCHANGE/MINIMIZEEND에서 style14ce0000·확장100·DWM1 그대로, 외곽1944×1144·내용1920×1080·여백52로 정상화. 외곽 원점958,683 유지, 내용 원점은970,735로 프레임만큼 변경. 24.329초 final도 정상 유지. 시간/활성화/DWM 변화로 복구된 것은 아니라는 비교 근거. 정확한 내부 WM_SIZE/WM_NCCALCSIZE 수신·게임 처리 방식은 캡처하지 않았으므로 특정 메시지가 원인이라고 단정 금지. DLL 주입·게임 subclass·입력·창 상태 쓰기 없음,60초 최대/256표본 한정 후 hook 해제.

공식 참조: [WinEvent 상수](https://learn.microsoft.com/en-us/windows/win32/winauto/event-constants), [WM_SIZE](https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-size), [WM_WINDOWPOSCHANGED](https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-windowposchanged). WinEvent는 상태 전환 관찰용이며 실제 내부 창 메시지 추적과 다름. 단순 resize와 minimize/restore 메시지 흐름이 같다고 가정하지 말 것.

후속 소스에는 일반 실행용 한정 작업 기록 추가: `%TEMP%/Bald-frame-state-<PID>.jsonl`, 512KiB 상한에서 순환. 적용 전 저장값 및 스타일/프레임 알림/DWM/완료·실패의 실제 좌표 기록. 제목·실행파일 경로·사용자 입력 없음. 진단 UI 모드·게임 조작 추가 없음. 아직 실행 중인 Bald에는 이 기록 코드가 없음.

1. 정상 창모드 상단바에서 새 방식 ON→OFF: 상단바 복구 및 게임 내용 크기 유지.
2. 상하좌우 드래그·화면 Y=0·해상도 변경: 점프/잘림/늘어남 없음.
3. 리스트 삭제·추가 취소·Bald 종료: 프레임 복구 및 게임 표시 유지.
4. Win+방향키: 게임/작업표시줄 사라짐 없음.

## 기록·GitHub

- 실행 파일: `target/debug/bald.exe`. 실제 게임에는 자동 실행/입력을 하지 않음.
- 과거 로그: `%TEMP%/Bald-window-diagnostics-<PID>.log`. 실험 자료 `scripts/`, `artifacts/`는 ignore. 자동 입력 스크립트 재실행 금지.
- 이전 검토 범위: 원격 main961cc8a 101파일, 로컬 이력21리비전. 검사한 비밀키/토큰 패턴·실행파일/진단 캡처 없음. 공개 업데이트 검증키는 비밀키 아님.
- 과거 UI 스크린샷18개가 Git 이력에 남음. 이미지 내용 전체 민감성 미검증, 이력 삭제는 별도 승인 필요.
- `target*`, `.reference`, `scripts`, `artifacts`, `.env`, `updater.key` ignore. 위 GitHub 결과는 이전 검사 범위이며 새 원격 검사 결과가 아님.
