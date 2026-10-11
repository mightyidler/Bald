# Bald 리소스·코드 감사

작성일: 2026-10-10, Asia/Seoul. 검토 대상: 작업 트리의 0.1.7 소스.

**결론: 최적화 여지는 있다. RAM은 WebView2 수명 관리, CPU·IPC는 UI 폴링과 전체 창 재검색을 먼저 다루는 것이 효과적인 후보다. 기능 오류 2개를 실제 Win32 검증 창에서 재현했으므로 이 수정이 먼저다.**

총 29개 항목: 리소스 개선 12개, 기능·안정성 문제 14개, 검증·정합성 문제 3개. 같은 원인에서 나오는 현상은 가능한 한 묶었다. 성능 개선율이나 절감 MB는 구현 전후 비교가 없어 제시하지 않는다.

## 범위와 판정 기준

Rust 서비스·감시·창 조작·설정·시작 프로그램·재실행, 프런트엔드 JS/HTML/CSS, 아이콘 처리, 빌드·배포 스크립트, CI와 기존 테스트를 검토했다. 설치 스크립트는 서비스·시작 프로그램 정리 경로 중심으로 확인했다. 실제 게임에 대한 자동 조작이나 설치·삭제·업데이트는 실행하지 않았다.

- **재현**: 원본 함수를 사용하는 별도 검증에서 현상을 관찰했다.
- **코드 확인**: 실행 경로와 조건을 소스에서 확인했다. 장애 발생 빈도는 측정하지 않았다.
- **측정 필요**: 변경 방향은 있으나 현재 병목이나 절감량은 아직 증명되지 않았다.
- **P1**: 잘못된 창 상태, 기능 중단, 설정 불일치 등 먼저 수정할 문제.
- **P2**: 반복 비용, 장애 대응, 검증 누락 등 다음 순서.
- **P3**: 상대적으로 작은 비용이나 정합성 개선.

애플리케이션 소스는 수정하지 않았다. 이 보고서와 `.gitignore` 대상인 `artifacts/audit-2026-10-10/`의 검증 자료를 생성했다.

## 참고 측정: 현재 실행 중인 설치본

실행 중인 `C:\Users\GUTS\AppData\Local\Bald\bald.exe`는 **0.1.6**이다. 현재 소스 0.1.7의 성능 측정으로 해석하면 안 된다. 설치본과 기존 로컬 release EXE의 SHA-256도 달랐다.

부모 PID를 따라 Bald 소속 WebView2 자식 6개만 집계했다. 다른 프로그램의 WebView2 프로세스는 제외했다. 20.01초 구간을 관찰했고, 직후 네이티브 조회에서 메인 창은 표시 중이며 최소화 상태가 아니었다. 관찰 중 상태를 강제로 고정하지 않았으므로 엄밀한 idle 벤치마크는 아니다.

| 대상 | 작업 집합 합계 | PrivateMemorySize64 합계 | CPU 누적 증가 |
| --- | ---: | ---: | ---: |
| Bald 본체 | 45.64 MiB | 11.91 MiB | 78.12 ms |
| 소속 WebView2 6개 | 380.41 MiB | 263.74 MiB | 203.14 ms |
| 합계 | 426.05 MiB | 275.65 MiB | 281.26 ms / 20.01초 |

작업 집합에는 프로세스 간 공유 페이지의 중복이 있을 수 있다. PrivateMemorySize64는 전용 커밋 메모리 참고치이며 실제 상주 RAM과 동일하지 않다. CPU 증가량은 전체 합계로 약 0.014개 논리 코어, 한 코어 기준 약 1.4%에 해당하지만 짧은 구간의 참고값이다. 이 숫자만으로 메모리 누수나 지속적인 CPU 과다 사용을 판단할 수 없다.

## 1. 리소스 개선 목록

### O01 · P2 · WebView2를 필요할 때 생성하고 닫은 뒤 해제하는 수명 관리

**코드 확인 + 측정 필요.** [tauri.conf.json](/C:/Projects/Bald/tauri.conf.json:13), [close_window](/C:/Projects/Bald/src/web_app.rs:388), [시작 경로](/C:/Projects/Bald/src/web_app.rs:466).

`visible:false`인 main 창을 시작 시 생성하고, 닫을 때 `hide()`만 수행한다. 트레이·창 감시만 필요한 동안에도 WebView 인스턴스가 유지되는 구조다. WebView2는 브라우저·렌더러·보조 프로세스 그룹을 사용하는 런타임이다. [Microsoft의 WebView2 프로세스 모델](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-model).

개선: 백그라운드 시작에서는 트레이와 서비스를 먼저 올리고, 설정 창을 열 때 WebView를 생성한다. 닫은 후 유예 시간을 두고 창을 실제 해제하는 방안을 비교한다. 가장 큰 RAM 절감 후보지만 다시 열기 지연, 창 위치·테마·picker 상태 복원, `show_existing()`의 HWND 탐색 방식도 함께 바꿔야 한다. suspend만으로 전체 프로세스 메모리가 사라진다고 가정하지 않는다. 네이티브 설정 UI로 교체하는 선택은 더 큰 구조 변경이므로 이 단계 이후에 비용을 비교한다.

검증: 동일한 0.1.7 빌드에서 표시·최소화·숨김·백그라운드 시작 각각의 프로세스 트리와 메모리를 비교한다. 트레이 및 두 번째 실행으로 다시 열리는지도 확인한다.

### O02 · P2 · 숨김 창의 2초 폴링을 중단하고 조회 중첩을 막기

**재현.** [타이머](/C:/Projects/Bald/web/app.js:376), [refresh](/C:/Projects/Bald/web/app.js:161), [get_state](/C:/Projects/Bald/src/web_app.rs:104).

picker가 열려 있는지만 검사하며 창의 표시·최소화 상태는 검사하지 않는다. `setInterval(async …)`은 이전 요청을 기다리지 않아서 응답이 2초보다 느리면 요청이 중첩된다. 느린 invoke를 사용한 원본 타이머 검증에서 숨김 상태에도 **미완료 요청 5개**가 동시에 발생했다. 실제 WebView가 숨김 타이머를 얼마나 늦추는지는 별도 측정 대상이다.

개선: 창 표시 이벤트와 서비스 상태 변경 이벤트로 갱신하고, 폴링을 남기면 완료 후 다음 타이머를 예약한다. 동시 요청은 1개로 제한한다. **자동 관리자 권한 요청도 현재 refresh에서 시작하므로**, 숨김 폴링을 제거할 때 해당 판단을 백엔드 서비스 이벤트로 옮겨야 한다.

검증: 숨김·최소화 중 UI용 invoke 0회, 다시 열 때 즉시 최신 상태 반영, 느린 응답에서도 동시 조회 1개, 숨김 상태의 관리자 대상 자동 적용 유지.

### O03 · P2 · 매번 전체 설정·아이콘을 전달하는 상태 응답 분리

**코드 확인.** [read_ui_state](/C:/Projects/Bald/src/web_app.rs:114), [아이콘 clone](/C:/Projects/Bald/src/web_app.rs:93), [렌더링](/C:/Projects/Bald/web/app.js:95).

조회마다 Config를 복제하고 모든 등록 앱의 base64 PNG 문자열을 복제·직렬화·전달한다. 캐시는 아이콘 추출을 줄일 뿐 전송 비용은 줄이지 않는다. 프런트엔드는 전체 HTML 문자열을 다시 만들고 이전 문자열과 비교하므로 DOM 교체를 피하더라도 문자열 생성·비교는 발생한다. `statuses`는 화면의 상태 표시에 쓰이지 않고 권한 필요 여부 판단에만 쓰인다.

개선: 초기/설정 변경 응답과 작은 상태 변경 응답을 분리한다. 아이콘은 최초 또는 파일 변경 시 전달하고 stable ID로 참조한다. icon 배열이 바뀌지 않으면 HTML도 다시 만들지 않는다.

검증: 등록 수 1/10/50개에서 상태 응답 byte 수와 JS 할당을 측정한다. 아이콘이 상태 변경 payload에 반복 포함되지 않아야 한다.

### O04 · P2 · WinEvent를 받은 HWND 중심으로 처리하고 연속 이동 이벤트를 제한

**코드 확인.** [callback](/C:/Projects/Bald/src/watcher.rs:140), [메시지 처리](/C:/Projects/Bald/src/watcher.rs:432), [scan](/C:/Projects/Bald/src/watcher.rs:527).

이벤트의 HWND가 있어도 요청에는 `()`만 보내 전체 창을 다시 열거한다. `LOCATIONCHANGE`는 등록 여부와 Bald 자체 드래그 여부만 검사하며 크기 변경과 단순 위치 변경을 구분하지 않는다. 이벤트 병합은 pending 메시지 1개 제한으로, 연속 이벤트의 초당 scan 횟수 제한은 아니다. 다른 앱의 CREATE/DESTROY/SHOW/HIDE와 MOVESIZEEND도 scan을 요청한다.

개선: 이벤트 HWND와 종류를 모아서 해당 창만 갱신한다. 위치만 바뀌었으면 region 크기 재검증을 생략하고, 크기 변경은 이전 치수와 비교한다. 소실 이벤트·HWND 재사용을 놓치지 않도록 **4초 reconciliation은 첫 단계에서 유지**한다. 이벤트 지연 예산을 정하고 연속 이벤트를 묶는다.

검증: 무관한 앱에서 창을 생성하거나 창을 계속 움직일 때 전체 EnumWindows 횟수를 기록한다. 등록 게임 생성·해상도 변경·최소화 복원 지연과 비교한다.

### O05 · P2 · 반복적인 창 설명·프로세스 경로·권한 조회 줄이기

**코드 확인.** [managed_windows](/C:/Projects/Bald/src/window_manager.rs:1482), [describe_window](/C:/Projects/Bald/src/window_manager.rs:2071), [enum_window](/C:/Projects/Bald/src/window_manager.rs:2112), [process_path](/C:/Projects/Bald/src/window_manager.rs:2148), [requires_elevation](/C:/Projects/Bald/src/window_manager.rs:2020).

scan은 `managed_windows()`가 만든 자세한 WindowInfo를 버린 뒤 `enumerate_windows()`에서 같은 창을 다시 설명한다. title도 enum callback과 describe에서 두 번 조회된다. 경로 조회마다 32,768개 u16, **64 KiB** 버퍼를 할당하고 OpenProcess/QueryFullProcessImageNameW를 호출한다. 이미 적용된 창에도 make_borderless 진입 전 권한 조회를 다시 수행한다.

개선: 관리 HWND 정리와 WindowInfo 생성 API를 분리한다. 스캔 내부에서 PID별 경로 조회를 공유하고 조회 버퍼를 재사용한다. 경로·권한을 장기 캐시하면 **PID 재사용 검증**을 추가해야 한다. Bald 자신의 승격 여부는 프로세스 수명 동안 고정된 값으로 취급할 수 있다.

검증: 한 프로세스에 여러 창이 있는 경우 PID별 경로 조회 1회, 이미 적용된 상태에서 반복 토큰 조회 감소, 종료/재실행 후 캐시 오적용 없음.

### O06 · P2 · 규칙 매칭에서 경로 문자열 재생성과 여러 전체 순회 줄이기

**코드 확인.** [normalize_path](/C:/Projects/Bald/src/rules.rs:72), [scan의 순회](/C:/Projects/Bald/src/watcher.rs:546).

tracking, drag rule, rule별 status 처리에서 규칙×창 비교를 반복한다. 경로 비교마다 `replace → trim → to_lowercase`로 양쪽 문자열을 다시 만든다. 일반적인 앱 수에서는 작은 비용일 수 있지만 창·규칙 수에 따라 커진다.

개선: 규칙 변경 시 정규화한 식별자를 준비하고, 한 스캔의 WindowInfo도 한 번 정규화한다. 한 번의 매칭 결과를 tracking/drag/status에서 공유한다. **경로 우선, 경로 불가 시 파일명+class fallback**이라는 기존 의미를 보존한다.

검증: 기존 규칙 테스트 유지, 1/10/50 규칙과 다중 창에서 할당 수·scan 시간 비교.

### O07 · P2 · 아이콘 캐시에 용량·실패 캐시·중복 추출 방지 추가

**코드 확인.** [icon_cache](/C:/Projects/Bald/src/web_app.rs:29), [cached_icon_data_url](/C:/Projects/Bald/src/web_app.rs:93), [list_windows](/C:/Projects/Bald/src/web_app.rs:142).

성공한 모든 실행 파일 아이콘은 종료까지 남으며 앱 제거나 picker 닫기에서 제거되지 않는다. 실패 결과는 저장하지 않아 파일이 사라진 등록 앱은 refresh마다 세 추출 경로를 다시 시도할 수 있다. 캐시 확인과 insert 사이에 잠금을 놓아 병렬 cache miss가 중복 추출할 수 있다.

개선: 등록 앱 캐시와 일시 picker 캐시를 구분하고 byte/entry 한도를 둔다. 실패는 짧은 TTL로 캐시하고 파일 변경 시 무효화한다. 경로별 진행 중 요청을 공유해 중복 디코딩을 줄인다. 전체 캐시 mutex를 잡은 채 파일·Shell 작업을 실행하지 않는다.

검증: 여러 앱을 실행/종료하며 picker를 반복 열었을 때 캐시 크기가 한도를 넘지 않는지, 삭제된 EXE의 재추출 빈도, 동시 miss의 추출 횟수를 확인한다.

### O08 · P2 · 아이콘 후보 이미지를 전부 보관하지 않기

**코드 확인.** [from_executable](/C:/Projects/Bald/src/executable_icon_resources.rs:72).

최대 256개 엔트리를 각각 디코딩한 뒤 RGBA 후보 전체를 Vec에 쌓고 마지막에 선택한다. 각 이미지의 최대 치수는 512×512이므로 이론적으로 후보 RGBA만 **256 MiB**에 도달할 수 있다. 보통 EXE가 이 크기를 실제 사용한다는 뜻은 아니다. 현재 디코더의 크기·할당 제한 자체는 좋은 방어다.

개선: 디코딩 → 투명 여백 제거 → 현재 최선 후보와 비교 → 불필요 후보 폐기를 반복한다. 원래의 **실제 cropped 픽셀 면적 기준**을 유지한다. resource header에 적힌 치수만 보고 선택하면 기존 품질 기준이 바뀐다.

검증: 선택 결과가 기존 아이콘 테스트와 같고, 많은 후보가 있는 EXE의 peak memory가 전체 후보 합계에 비례하지 않는지 확인한다.

### O09 · P2 · 창 선택 목록 생성과 PNG 인코딩을 UI 스레드에서 분리

**코드 확인.** [동기 list_windows](/C:/Projects/Bald/src/web_app.rs:141), [add_application의 열거](/C:/Projects/Bald/src/web_app.rs:197), [PNG 생성](/C:/Projects/Bald/src/web_app.rs:80).

list_windows는 동기 Tauri command에서 전체 창 열거, EXE resource/Shell 아이콘 추출, PNG 인코딩을 수행한다. 등록 함수도 async 선언 후 첫 await 전에 전체 열거를 수행한다. 반면 get_state는 이미 spawn_blocking을 사용한다.

개선: 목록 생성도 blocking worker에서 수행한다. 제목·식별자를 먼저 반환하고 아이콘은 별도로 채우는 방안을 비교한다. 목록에 동일 EXE의 여러 창이 있으면 추출을 공유한다.

검증: cold cache와 warm cache의 picker 열기 시간, 처리 중 창 이동·닫기 응답성을 비교한다.

### O10 · P2 · 언어·테마 변경에서 네이티브 서비스 동기화와 전체 스캔 생략

**코드 확인.** [persist](/C:/Projects/Bald/src/web_app.rs:156), [set_language/set_theme](/C:/Projects/Bald/src/web_app.rs:290), [service.sync](/C:/Projects/Bald/src/border_service.rs:197).

언어·테마·시작 프로그램 변경도 공통 persist가 전체 Config를 서비스에 전송하고 watcher.notify를 호출한다. 표시 설정 하나를 바꿀 때 네이티브 창 검색과 적용 검사가 이어진다. 저장은 async 함수 안에서 동기 파일 write/sync_all을 수행하며 read guard도 그동안 잡는다.

개선: UI 표시 설정 저장과 창 동작 설정 동기화를 분리한다. 저장 I/O는 직렬화된 blocking 작업으로 옮긴다. 디스크 flush를 무조건 없애는 대신 B05/B06의 트랜잭션·동시 저장 문제를 함께 해결한다.

검증: 언어·테마 변경에서 서비스 sync/EnumWindows 0회, 디스크 저장 성공·실패 결과의 UI 일치.

### O11 · P3 · 약 6 MiB의 네 가지 OTF 폰트와 preload 최적화

**코드 확인 + 측정 필요.** [폰트 선언](/C:/Projects/Bald/web/style.css:1), [preload](/C:/Projects/Bald/web/index.html:2), [폰트 대기](/C:/Projects/Bald/web/app.js:301).

OTF 네 파일 합계는 **6,318,784 bytes, 약 6.03 MiB**다. 세 weight는 시작 시 preload하고 content reveal도 세 weight의 준비를 기다린다. 작은 설정 창으로서는 번들·cold start 최적화 후보다. 압축 파일 byte를 그대로 RAM 절감량으로 해석하면 안 된다.

개선: 라이선스를 유지하면서 WOFF2·variable font·실사용 문자 subset을 비교한다. 한글뿐 아니라 동적으로 들어오는 앱 제목과 영어가 누락되지 않아야 한다. 백그라운드 시작에서 UI를 생성하지 않는 O01이 선행하면 이 초기 로딩도 지연된다.

검증: 설치/EXE 크기, cold start 첫 표시 시간, 한글·영어·앱 제목 렌더링.

### O12 · P3 · 반복 행의 상시 will-change와 blur 합성 비용 측정

**코드 확인 + 측정 필요.** [아이콘/text swap](/C:/Projects/Bald/web/style.css:136), [버튼](/C:/Projects/Bald/web/style.css:157), [모달](/C:/Projects/Bald/web/style.css:437).

picker의 각 행 아이콘·텍스트·버튼에 `will-change`가 상시 선언되어 있고 여러 표면에 blur/backdrop-filter가 있다. 일부 레이어의 장기 유지 비용은 행 수에 따라 커질 수 있다. MDN도 상시·과다 will-change의 메모리 비용을 설명한다. [MDN will-change](https://developer.mozilla.org/en-US/docs/Web/CSS/Reference/Properties/will-change).

개선: WebView 렌더링 trace에서 병목이 확인된 표면만 조정한다. 애니메이션 직전과 직후의 힌트 수명 관리를 비교하고 저비용 opacity/transform 전환을 검토한다. 현재 stagger는 완료 후 `will-change:auto`로 정리하고 닫힌 패널은 visibility를 끄므로 이 부분은 이미 개선되어 있다. transitions 스킬의 레시피를 무조건 삭제하거나 모든 효과를 꺼서 처리할 필요는 없다.

검증: picker 행 수별 GPU/renderer 메모리, scrolling frame time, 테마·reduced-motion 동작을 비교한다.

## 2. 기능·안정성 문제 목록

### B01 · P1 · 테두리 복원 시 살아 있는 WS_DISABLED 상태를 덮어씀

**재현.** [style 복원](/C:/Projects/Bald/src/window_manager.rs:1337).

일반 style 복원은 원래 style에서 visible/minimized/maximized에 해당하는 `0x3100_0000` 비트만 현재 상태로 보존한다. 앱이 테두리 제거 이후 모달 등을 띄우며 부모 창을 비활성화한 경우, 저장 시점의 enabled 상태가 복원되어 **비활성 창을 다시 활성화**한다. 별도 Win32 창에서 `make_borderless → EnableWindow(false) → restore_borders`로 재현했고 WS_DISABLED가 제거되어 IsWindowEnabled가 true로 바뀌었다.

수정: Bald가 실제 변경한 BORDER_STYLES 비트만 현재 style에 병합한다. 원래 style 전체를 복원해야 하는 호환 경로는 별도로 검증한다. extended style은 이미 변경한 frame 비트만 복원하는 방식이 있다.

회귀 검증: 비활성화·다른 runtime style 변경·모달 부모 상태를 유지하면서 테두리가 복원되어야 한다.

### B02 · P1 · 미완료 드래그 중 숨김/최소화에서 in_flight가 남아 이후 드래그를 차단

**재현.** [poll_user_drag](/C:/Projects/Bald/src/window_manager.rs:1689), [worker의 gesture 폐기](/C:/Projects/Bald/src/watcher.rs:265), [시작 조건](/C:/Projects/Bald/src/window_manager.rs:382).

poll은 창이 숨겨지거나 최소화되면 `Ok(false)`로 종료하지만 해당 rule의 in_flight를 정리하지 않는다. worker는 gesture를 버리므로 이후 완료 확인도 사라진다. 표시 복원 후 configure_drag_rules는 같은 PID의 상태를 유지하며 mode 변경도 stalled만 초기화한다. 검증 창에서 **hide → poll(false) → show 이후 시작 불가, mode를 차단/허용으로 바꿔도 시작 불가**를 재현했다.

수정: gesture 종료 사유별로 pending OS 요청의 수명을 정리한다. 이미 게시된 요청은 취소되지 않으므로 단순히 in_flight를 즉시 false로 만들고 새 요청을 보내면 충돌할 수 있다. pending 요청을 계속 확인하거나, 안전한 종료/timeout을 거친 후 새 gesture를 허용해야 한다.

회귀 검증: 이동 중 숨김·최소화·종료·권한 변경·API 실패 각각에서 stale flag가 남지 않고 한 개 outstanding request 원칙을 유지한다.

### B03 · P2 · 버튼 해제 시 미완료 이동이 있으면 최종 커서 좌표 유실

**코드 확인.** [DragEvent::End](/C:/Projects/Bald/src/watcher.rs:312), [ending 시 polling](/C:/Projects/Bald/src/watcher.rs:265).

End(point)는 pending이 없을 때만 point를 적용한다. pending이 있으면 최종 point를 저장하지 않고 ending=true로 전환한다. 기존 위치 요청이 완료되면 gesture를 없애므로 release 위치에 도달하지 못할 수 있다. 예: 10px 이동 요청이 미완료인 동안 커서가 30px에 도달하고 해제되면 10px 위치에서 끝날 수 있다.

수정: 최종 목표를 보관하고 기존 요청이 확인된 뒤 최종 좌표 요청 한 번을 처리한다. 취소·차단 변경에서는 최종 이동을 실행하지 않도록 구분한다.

회귀 검증: 지연 owner fixture에서 Move → End → 이전 request ack 순서의 최종 위치와 request 개수를 확인한다.

### B04 · P1 · picker에서 선택한 창을 title+실행 파일명만으로 다시 찾음

**코드 확인.** [WindowChoice](/C:/Projects/Bald/src/web_app.rs:58), [add_application](/C:/Projects/Bald/src/web_app.rs:197).

choice에 경로와 class가 있어도 재열거 결과에서 title와 executable_name만 비교한다. 같은 파일명을 가진 다른 경로의 앱 또는 같은 제목의 main/splash 창 중 첫 번째가 등록될 수 있다. 제목만 바뀌어도 살아 있는 선택 창을 찾지 못한다.

수정: HWND와 PID, 실행 경로·class를 선택 식별자로 전달하고 등록 시 살아 있는 동일 대상인지 확인한다. HWND/PID 재사용도 고려한다. 실행 중 창의 식별자는 세션용이며 설정에 영구 저장하지 않는다.

회귀 검증: 같은 제목·같은 exe basename·서로 다른 경로/class, 선택 후 제목 변경, 선택 후 종료.

### B05 · P1 · 설정 변경 후 저장/서비스 적용 실패 시 상태가 갈라짐

**코드 확인.** [persist](/C:/Projects/Bald/src/web_app.rs:156), [set_application_drag_mode](/C:/Projects/Bald/src/web_app.rs:245), [remove_application](/C:/Projects/Bald/src/web_app.rs:217).

메모리 설정을 먼저 바꾸고 저장한다. 저장 실패 시 이전 값으로 되돌리지 않는다. 일반 persist는 저장 결과가 Err여도 서비스 sync를 실행하므로 디스크는 이전 설정인데 서비스는 새 설정일 수 있다. drag mode는 저장 실패 시 sync 전에 반환하므로 UI 메모리는 새 mode인데 서비스는 옛 mode일 수 있다. remove/automatic OFF는 복원 실패여도 저장을 진행한다.

수정: 변경 후보 → 저장·서비스 적용 → commit 순서를 명시하고 실패 정책을 정한다. 특히 복원 실패 후 등록 목록에서 사라진 창을 어떻게 재시도할지 보존해야 한다. UI가 읽은 메모리 값을 '저장된 상태'로 오인하지 않도록 부분 실패를 표현한다.

회귀 검증: 디스크 쓰기 거절·서비스 단절·창 복원 실패를 주입해 UI/메모리/디스크/worker의 상태와 재시도를 비교한다.

### B06 · P1 · 동시 저장에서 같은 config.json.tmp를 공유

**코드 확인, 동시 장애 미재현.** [save](/C:/Projects/Bald/src/config.rs:59), [shared read guard에서 저장](/C:/Projects/Bald/src/web_app.rs:156).

Config::save는 항상 동일한 `config.json.tmp`를 File::create로 열어 쓴다. persist는 공유 read lock 아래 저장하므로 저장 자체를 직렬화하지 않는다. 동시 command가 저장하면 truncate/write/rename 간 충돌 또는 rename 실패가 가능하다. 단일 저장의 atomic rename만으로 동시 저장 안전성이 증명되지 않는다.

수정: 전체 설정 mutation과 save를 한 순서로 처리하는 직렬 저장 경로를 둔다. 고유 임시 파일을 쓰더라도 오래된 snapshot이 최신 파일을 덮지 않도록 commit 순서를 보장한다.

회귀 검증: 격리된 config 경로에서 동시에 여러 command를 실행해 최종 JSON 파싱과 마지막 요청의 값, 임시 파일 정리를 확인한다. 실제 사용자 설정에는 장애를 주입하지 않는다.

### B07 · P1 · 보조 서비스 단절/timeout 후 remote를 계속 재사용

**코드 확인.** [Remote::request](/C:/Projects/Bald/src/border_service.rs:115), [statuses](/C:/Projects/Bald/src/border_service.rs:209).

연결 오류가 나도 remote 상태를 제거하거나 unhealthy로 표시하지 않는다. 이후 polling과 설정 변경도 같은 연결을 사용한다. `read_exact`가 프레임 일부를 읽고 timeout되면 다음 request가 같은 stream에서 새 length를 읽어 프로토콜이 어긋날 가능성도 있다. UI 서비스 mutex를 통신 동안 유지하며 timeout은 최대 60초다.

수정: 전송/수신 실패 시 연결을 종료하고 일관된 stopped 상태를 알린다. 응답 correlation과 요청 deadline을 명시한다. 복구하려면 B08의 소유권 이전과 Watcher의 전역 OnceLock 수명까지 같이 설계해야 한다. 프레임 소유권이 살아 있을 수 있으므로 무조건 local watcher를 재시작하지 않는다.

회귀 검증: worker 강제 종료, 응답 지연, 잘린 response, reconnect 후 상태·설정·창 원복을 검증한다.

### B08 · P1 · 관리자 서비스 Initialize 실패 시 기존 local 소유자가 이미 사라짐

**코드 확인.** [elevate](/C:/Projects/Bald/src/border_service.rs:259).

연결 성공 뒤 local 창을 복원·중단하고 local=None, remote=Some으로 바꾼 다음 Initialize를 요청한다. Initialize 실패 시 기존 local로 복구하지 않는다. remote가 Some이므로 다음 elevate도 즉시 Ok로 종료한다.

수정: 연결과 초기화 준비 상태를 분리하고, 원래 소유자 해제 후 새 소유자 시작이라는 순서를 보존하면서 실패를 복구할 수 있도록 한다. 두 watcher를 동시에 적용 상태로 만들면 안 된다.

회귀 검증: handshake 성공 직후 단절, Initialize error/timeout, 재시도, 정상·실패 전환 중 프레임 소유자가 중복되지 않는지 확인한다.

### B09 · P1 · 동기 종료·복원과 5초 교체 대기의 충돌

**코드 확인, 지연 종료 미재현.** [restore_before_exit](/C:/Projects/Bald/src/web_app.rs:35), [재실행](/C:/Projects/Bald/src/web_app.rs:364), [새 프로세스 대기](/C:/Projects/Bald/src/main.rs:47), [복원 대기](/C:/Projects/Bald/src/window_manager.rs:1943).

exit command와 tray callback은 shutdown을 동기로 호출한다. 일반 창 하나의 복원 확인은 최대 5초이며 여러 창을 순차 처리한다. remote 요청은 최대 60초다. 일부 타 프로세스 SetWindowPos는 동기 호출이라 함수 내부에서 지연되면 자체 polling timeout에 도달하기 전부터 막힐 수 있다. [SetWindowPos의 ASYNCWINDOWPOS 동작](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowpos).

업데이트 재실행은 새 프로세스를 먼저 spawn하고 기존 프로세스를 복원한다. 새 프로세스의 primary 대기는 5초뿐이므로 기존 복원이 이를 넘으면 새 프로세스가 종료되고, 이후 기존 프로세스도 종료되어 Bald가 다시 실행되지 않을 수 있다. 종료 단계 여러 곳에서 restore_before_exit가 호출되어 실패 시 같은 복원을 반복할 수도 있다.

수정: 종료/재시작 상태를 하나로 관리하고 복원 작업을 UI thread에서 분리한다. 새 프로세스와 종료 완료 handshake를 두거나 시간 예산을 일관되게 설계한다. 단순히 timeout을 줄이거나 외부 창 조작에 무조건 ASYNC flag를 붙이면 owner geometry 확인과 복원 정확도가 바뀐다.

회귀 검증: 지연·응답 없는 창 여러 개, remote 지연, 사용자 종료, 업데이트 재실행 후 primary 인스턴스 1개 유지.

### B10 · P2 · UAC 취소/일시 실패 후 자동 승격 재시도 경로 없음

**코드 확인.** [elevationRequested](/C:/Projects/Bald/web/app.js:5), [승격 호출](/C:/Projects/Bald/web/app.js:163).

호출 전에 true로 바꾸고 실패해도 되돌리지 않는다. 설정 창이 재생성되거나 프로세스가 재시작되기 전에는 같은 requires_elevation을 자동 처리하지 않는다. 오류 메시지는 있으나 명시적인 retry UI가 없다.

수정: 필요/요청 중/거절/실패/연결 상태를 구분한다. 취소 직후 매 2초마다 UAC를 다시 띄우지 말고 사용자 재시도 동작이나 상태 변화로 재시도한다.

회귀 검증: 취소 후 일반 앱 동작 유지, 재시도 가능, prompt 반복 방지.

### B11 · P2 · 실행 중 설정 창의 비동기 실패가 여러 경로에서 처리되지 않음

**코드 확인.** [picker](/C:/Projects/Bald/web/app.js:147), [startup/reset/language/theme handlers](/C:/Projects/Bald/web/app.js:244), [초기 로드](/C:/Projects/Bald/web/app.js:374).

picker toggle에는 finally만 있고 catch가 없다. 시작 프로그램·reset·언어·테마 handler도 실패 후 안내와 상태 재조회가 빠진다. periodic refresh rejection도 catch가 없다. 초기 Promise.all은 get_state 실패도 버전 표시 '-'로 뭉개며 state가 준비되지 않은 화면을 열 수 있다. drag mode/automatic 변경에는 이미 오류 처리 예제가 있다.

수정: 공통 요청 오류 경로와 pending 상태 정리를 사용한다. 실패한 설정은 실제 authoritative state로 다시 표시하고 초기 로드 실패는 재시도 가능한 상태를 보여준다.

회귀 검증: 각 invoke reject에서 unhandled rejection 0건, 버튼 복구, 실제 저장/서비스 상태와 표시 일치.

### B12 · P2 · 시작 시 자동 업데이트와 수동 업데이트의 앱 수준 직렬화 없음

**코드 확인, 동시 installer 실행 미재현.** [자동 업데이트](/C:/Projects/Bald/src/web_app.rs:444), [수동 업데이트](/C:/Projects/Bald/src/web_app.rs:428), [setup의 spawn](/C:/Projects/Bald/src/web_app.rs:503).

시작 시 설치 흐름을 spawn하는 동시에 UI에서 수동 설치를 시작할 수 있다. 수동 버튼 disabled는 자동 작업과 공유되지 않는다. 현재 앱 코드에는 check/download/install/restart를 묶는 공통 진행 상태가 없다. installer 또는 라이브러리 내부 제약이 최종 중복 실행을 막는지는 이번 검토에서 입증하지 않았다.

수정: 단일 update coordinator가 진행 상태와 restart를 소유하고 수동 요청은 진행 중 결과에 합류하게 한다. 사용자 안내 text도 periodic applyLanguage가 '확인 중'에서 '업데이트 확인'으로 되돌리지 않도록 처리한다.

회귀 검증: 지연 업데이트 확인 중 수동 클릭 시 download/install/restart 경로 1회, 오류·취소 후 재시도.

### B13 · P2 · 네이티브 marker 실패와 region 임시 핸들의 정리가 불완전

**코드 확인, API 실패 주입 미실시.** [style marker](/C:/Projects/Bald/src/window_manager.rs:643), [region 생성](/C:/Projects/Bald/src/window_manager.rs:739), [SetPropW 오류 반환](/C:/Projects/Bald/src/window_manager.rs:793).

일반 적용은 원복 ownership 확인에 사용하는 SetPropW 결과를 무시한다. marker 설정이 실패해도 style은 바뀔 수 있고 다음 managed_windows는 marker 부재로 원복 정보를 잊는다. native clipping은 desired/previous region을 만든 뒤 SetPropW에 `?`를 사용하므로 그 지점의 실패는 아래 임시 handle 정리에 도달하지 않는다. 수동 DeleteObject가 많은 코드에서 부분 실패 수명 관리가 취약하다.

수정: Win32 region/handle에 RAII guard를 두고, OS에 소유권을 넘긴 성공 시에만 guard를 해제한다. marker를 확인한 뒤 mutation하고 부분 적용 실패의 rollback·원복 정보 보존을 검증한다.

회귀 검증: SetPropW/DWM/SetWindowRgn 실패 주입과 GDI object count, 원복 snapshot/marker 일관성.

### B14 · P2 · 시작 프로그램·관리자 예약 작업 수명과 수동 열기 의미 불일치

**코드 확인.** [main의 우선 분기](/C:/Projects/Bald/src/main.rs:36), [예약 작업 생성·실행](/C:/Projects/Bald/src/startup.rs:33), [시작 프로그램 등록](/C:/Projects/Bald/src/startup.rs:100), [uninstaller 정리](/C:/Projects/Bald/tools/windows/installer.nsi:876).

release에서는 single-instance 처리보다 먼저 예약 작업을 실행하고 성공하면 반환한다. 예약 작업 인자는 항상 `--elevated-task --background`라 수동 실행의 '창 열기' 요청이 전달되지 않으며, 이미 실행 중인 작업에 대한 /Run 성공만으로 UI 활성화가 보장되지 않는다. 향후 실행에서는 창 전용 worker 대신 UI 전체가 관리자 권한으로 시작하는 경로도 남아 있다.

또한 set_enabled(false)는 없는 Run 값의 삭제를 실패로 처리하여 idempotent하지 않다. uninstaller는 Run 값을 정리하지만 코드에 고정된 `Bald Elevated` 예약 작업 삭제 경로는 찾지 못했다. 등록 실패·시작 등록 실패 일부는 호출자가 무시한다.

수정: 수동 열기와 자동 시작의 목적을 분리하고 기존 인스턴스 활성화를 먼저 처리한다. 필요 권한과 예약 작업 정책을 정하고 uninstall에서 자신이 만든 해당 경로의 작업만 정리한다. 이미 꺼진 startup 삭제는 성공으로 취급한다.

회귀 검증: 예약 작업 유무·실행 중 여부·수동/자동 시작·시작 프로그램 OFF·제거 후 잔여 작업을 release 환경의 격리된 app identity에서 확인한다.

## 3. 검증·정합성 문제

### Q01 · P1 · 전체 Rust 테스트의 반복 실패와 검증 범위 누락

**실행 확인.** [Maple 테스트](/C:/Projects/Bald/src/window_manager_windows_tests.rs:1061), [실제 마우스 상태 참조](/C:/Projects/Bald/src/window_manager.rs:710), [격리 worker 테스트](/C:/Projects/Bald/src/border_service.rs:478).

`cargo test -- --test-threads=1` 첫 실행: **55 통과, 1 실패, 2 ignored**. 실패는 maple_application_path_preserves_styles_and_restores_modern_rendering의 native rendering restoration 확인이다. 동일 테스트 단독 실행은 1/1 통과했다.

두 번째 전체 실행: **53 통과, 3 실패, 2 ignored**. 실패는 maple_application_path_preserves_styles_and_restores_modern_rendering, maple_drag_does_not_allow_position_callback_to_pin_y_to_zero, native_region_preserves_styles_content_and_restores_each_cycle다. 첫 항목은 apply가 false, 둘째는 drag 생성 None, 셋째는 DWM rendering 확인 timeout이었다.

원인은 확정하지 않았다. native 경로는 실제 desktop DWM와 전역 GetAsyncKeyState를 사용하므로 사용자 입력·환경에 따라 테스트가 흔들릴 여지가 있다. 이 결과가 실제 게임의 동일 오류를 증명하지는 않으며, 단독 통과로 전체 실패를 지우지도 않는다. B01/B02를 기존 테스트가 놓친 점도 확인했다.

개선: 고정 input/frame 조건과 실패 사유를 구분하는 fixture를 만들고 B01/B02/B03 회귀 검증을 추가한다. worker lifecycle은 ignored 상태라 일반 cargo test만으로 검증되지 않으므로 별도 CI 단계가 필요하다.

### Q02 · P2 · 배포 CI에 lint·JS 테스트·Rust 테스트 gate 없음

**코드 확인.** [release workflow](/C:/Projects/Bald/.github/workflows/release.yml:18), [release script](/C:/Projects/Bald/bump-and-push.js:47), [package scripts](/C:/Projects/Bald/package.json:5).

release workflow는 빌드와 installer lifecycle 검증 후 게시하지만 npm lint, JS 테스트, Rust 단위/회귀 테스트를 실행하지 않는다. 버전 bump 스크립트도 cargo check만 실행한다. package.json에 test script가 없어서 자동화가 테스트를 발견하기 어렵다.

개선: 빌드 전 lint와 JS 테스트, Rust 검사, 안정화한 native 테스트를 넣는다. worker isolated test는 별도 실행한다. 현재 실패하는 검증을 suppress해서 통과시키지 않는다.

검증: 테스트 실패를 주입한 태그에서 배포가 publish 단계 전에 중단되어야 한다.

### Q03 · P3 · 문서·버전 metadata·Rust 정적 검사 정합성

**코드 및 실행 확인.** [drag 상수](/C:/Projects/Bald/src/window_manager.rs:280), [문서](/C:/Projects/Bald/docs/IMPLEMENTATION.md:54), [bump script](/C:/Projects/Bald/bump-and-push.js:39), [빈 else](/C:/Projects/Bald/src/window_manager.rs:832).

문서는 드래그 strip을 12 physical pixels로 설명하지만 코드와 테스트는 **16px**다. 문서의 'ordinary movement는 scan 요청 없음'도 O04의 callback과 다르다. README의 per-app enable switch는 설정 필드만 존재하고 현재 UI/command에서 조작 경로를 찾지 못했다.

버전 bump는 package.json/tauri.conf/Cargo.toml을 변경하지만 package-lock.json의 root version을 갱신하는 단계 없이 stage한다. version metadata가 뒤처질 수 있다. 이는 곧바로 npm ci가 실패한다는 주장과는 별개다.

`cargo clippy --all-targets -- -D warnings`는 실패했다. 일반 binary에서 **14개**, test target에서 이를 포함해 **25개** 진단이 나왔으며 empty else, collapsible_if, let_and_return, default 뒤 필드 재할당, test의 dangling pointer 표기 등이 포함된다. 대부분 스타일 진단이며 이 숫자를 런타임 버그 수로 취급하지 않는다. npm lint와 cargo fmt는 통과했다.

개선: 실제 동작에 맞춰 문서를 갱신하고 모든 version source를 함께 업데이트한다. 스타일 정리는 기능 수정과 별도 변경으로 묶고 rules를 약화하지 않는다.

## 이미 비용을 줄인 구조: 유지할 것

- watcher는 GetMessage로 대기하며 상시 16ms busy polling을 하지 않는다.
- 이벤트 wake와 mouse burst는 pending 1개로 병합한다. 드래그 좌표는 latest 값으로 대체한다.
- 드래그의 16ms 확인은 outstanding move가 있을 때만 진행하고 timeout을 둔다.
- border 제거는 반복 적용으로 원복을 토글하지 않으며, 이미 borderless인 일반 창에서 빠르게 종료한다.
- 마우스 hook은 무관한 move를 빠르게 통과시키고 창 조작은 별도 worker에서 수행한다.
- 이미 등록된 대상이 없으면 full EnumWindows를 건너뛴다.
- 성공 아이콘 추출 캐시와 변경 없는 applications HTML의 DOM 교체 방지가 있다.
- 아이콘 resource를 data/resource flags로 읽으며 target EXE를 실행하지 않는다. 크기 제한도 있다.
- release profile은 opt-level=s, LTO, codegen-units=1, panic=abort, strip을 이미 사용한다. 이 옵션을 다시 켜는 제안은 추가 최적화가 아니다.
- style·region·geometry·PID와 HWND 소유권을 다루는 호환성 방어가 있으므로 CPU 절감 목적으로 확인 절차를 일괄 제거하지 않는다.

## 권장 실행 순서와 성공 기준

1. **정확성부터**: B01/B02를 회귀 테스트와 함께 수정하고 B03/B04를 처리한다. 비활성 창 유지, hide/show 뒤 재드래그, 정확한 release 위치·선택 대상이 기준이다.
2. **설정·서비스 일관성**: B05~B10/B13을 처리한다. 저장·통신·승격·종료 실패에서도 authoritative state와 프레임 소유자가 분명해야 한다.
3. **작은 리소스 변경**: O02/O03/O05/O06/O07/O10을 적용한다. 숨김 UI 호출 0, 동시 조회 1, 아이콘 재전송 없음, PID별 조회 공유가 기준이다.
4. **큰 RAM 변경**: O01을 별도 실험한다. 같은 release build에서 WebView 미생성/해제 후 메모리와 재오픈 시간을 비교한다.
5. **후순위 비용**: trace를 보고 O04/O08/O09/O11/O12를 선택한다. 실제 사용의 창·규칙 수에서 의미 있는 변화가 확인될 때 진행한다.
6. **배포 보장**: Q01~Q03을 정리하고 자동 gate를 추가한다. 린트 통과만으로 Rust·Win32 runtime 통과를 대신하지 않는다.

### 측정 시나리오

동일 0.1.7 release build와 동일 WebView2/runtime 환경에서 규칙 0/1/10/50개를 비교한다. 앱 창 표시, 최소화, 트레이 숨김, 자동 제거 OFF, 대상 게임 미실행, 대상 창 이동/해상도 변경, picker cold/warm, 승격 worker 유무를 구분한다. cold start와 warm steady-state도 구분한다.

작업 집합·private commit·자식 프로세스 수·CPU 시간·스레드/handle/GDI object 수·초당 scan/invoke 횟수·상태 payload byte·picker 지연·재오픈 시간·창 복원 성공 여부를 기록한다. 실게임 FPS/input 영향과 호환성은 사용자가 승인한 별도 실게임 검증에서 확인해야 한다. 이번 감사는 실제 게임을 조작하지 않았다.

## 실행한 검증과 남은 실패

- `npm run lint`: exit 0.
- `node --test tests/*.test.cjs`: exit 0, 17/17 통과.
- `cargo fmt --check`: exit 0.
- `cargo check --release`: exit 0. 타입·컴파일 검사이며 release installer를 새로 빌드한 결과는 아니다.
- `cargo test -- --test-threads=1`: 첫 실행 exit 1, 55/1/2; 두 번째 실행 exit 1, 53/3/2.
- `cargo test window_manager::windows_tests::maple_application_path_preserves_styles_and_restores_modern_rendering -- --exact --test-threads=1`: exit 0, 1/1 통과.
- `cargo test border_service::tests::worker_process_keeps_ui_and_restores_on_disconnect -- --exact --ignored --test-threads=1 --nocapture`: exit 0, parent와 child 각각 통과.
- `cargo clippy --all-targets -- -D warnings`: exit 1. production 14개, test target 25개 진단.
- 감사용 native probe 2개: exit 0. **오류 현상을 확인하는 assertion이 통과했다는 뜻이며 앱이 정상이라는 뜻이 아니다.**
- 감사용 UI polling probe: 숨김 상태 미완료 invoke 5개 재현.
- `git diff --check`: exit 0.

감사용 probe는 [검증 폴더](/C:/Projects/Bald/artifacts/audit-2026-10-10/)에 있다. `node artifacts/audit-2026-10-10/ui-poll-probe.cjs`와 `powershell -NoProfile -File artifacts/audit-2026-10-10/run-native-probes.ps1`로 실행했다. Native runner는 이번 cargo test의 fingerprint 경로를 사용하므로 빌드 구성 변경 후에는 경로를 갱신해야 한다.

**현재 판정: 소스 전반의 개선 목록과 재현 근거는 확보했지만, 전체 Rust 테스트 및 strict Clippy는 통과하지 않는다. 실제 절감량은 수정 후 같은 버전으로 비교 측정해야 한다.**
