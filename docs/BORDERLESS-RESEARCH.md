# 테두리 제거·이동 구현 비교

2026-10-05. 공개 소스의 아래 커밋을 직접 확인. 상용 최신판·메이플 호환성을 입증하는 자료는 아님. 원본 코드를 복사하지 않고 Win32 동작과 설계를 비교했다.

| 소스 | 테두리·복구 | 이동 관련 차이 |
| --- | --- | --- |
| [Borderless Gaming](https://github.com/andrewmd5/Borderless-Gaming/blob/d6a541a64a2a9baf0f4aae55434a21fd9c93b8aa/BorderlessGaming.Logic/Windows/Manipulation.cs) | 일반/확장 스타일 변경. 원래 스타일·외곽 저장 후 복구. 영역 자르기 없음. | 적용에 NOSENDCHANGING 사용. Unreal/GameMaker 클래스는 적용 순서·지연 예외가 있음. 자유 드래그 해결을 제공한다는 근거는 아님. |
| [SRWE](https://github.com/dtgDTGdtg/SRWE/blob/ec048a8ee833122fc1eb893ef8324d9ed315a0a6/SRWE/Window.cs) | 일반 프레임 및 확장 프레임 4종 제거. FRAMECHANGED 요청. | 적용/위치 변경에 NOSENDCHANGING, NOACTIVATE, NOOWNERZORDER 사용. |
| [AltDrag](https://github.com/stefansundin/altdrag/blob/e2740d605b0336a3b391fec26794718864b19521/hooks.c) | 테두리 제거 제품이 아니라 이동 구현 참고. | 클릭 지점과 창 원점의 차이를 저장해 좌표 직접 변경. 기본 SC_MOVE에 의존하지 않음. |

## Bald에 반영한 변경

- 메이플 클래스별 영역 자르기를 신규 적용 경로에서 제거. 모든 등록 창에 동일한 스타일 방식 사용.
- 일반 프레임 + 확장 프레임 4종만 제거·복구. APPWINDOW/TOOLWINDOW/LAYERED/TOPMOST 등의 무관한 비트는 유지.
- 적용·복구 위치 요청에 NOSENDCHANGING/NOOWNERZORDER/NOACTIVATE/NOZORDER 적용. [NOSENDCHANGING은 대상의 WM_WINDOWPOSCHANGING을 생략한다](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowpos). 사용자 드래그는 owner의 위치 제한을 우회하지 않도록 NOSENDCHANGING 제외. 메이플의 실제 점프 원인이 이 메시지라는 결론은 아직 미검증.
- 반복 복구 후속: 이동/내용 크기 변경과 프레임 갱신 알림을 분리. 알림은 NOMOVE/NOSIZE로 위치를 유지하면서 NOSENDCHANGING 제외. 알림 후 실제 프레임을 측정해 내용 크기 보존.
- 원래 DWM 비클라이언트 렌더링이 켜져 있던 창은 복구 시 그 상태도 확인·복원. [DWMWA_NCRENDERING_ENABLED는 읽기, POLICY는 쓰기 속성](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwmwindowattribute). 원래 정책 값 자체를 읽어 복원했다고 주장하지 않음.
- 드래그는 고정 클릭 기준 좌표 이동으로 변경. 40ms 대기·SC_MOVE 제거. 입력 이벤트 기반 별도 worker, 이동 이벤트 병합, 평소 polling 없음.
- 이동 확인/명시적 허용된 활성 borderless 창 상단 클릭/해제만 소비. 마우스 이동 자체는 통과. 훅에서 게임 hit-test 메시지 전송·대기 잠금 제거, 창 위치 쓰기는 worker에서만 수행. 최신 좌표 1개·미반영 요청 1개 제한, 100ms 미반영 시 해당 창 드래그 중단. 실제 게임 체감 지연은 별도 검증 필요.
- OFF/삭제/종료 중 관리 상태가 없어지면 이동 중지. 창 숨김·최소화·강제 활성화 없음.
- 복구는 실제 상단 여백을 확인. 스타일 비트만 돌아온 경우 실패로 표시하고 관리 상태 유지.
- 구버전 자르기 마커는 게임 자체 창모드 재설정으로 프레임이 이미 정상 복구된 경우에만 정리. 미복구 상태를 새 방식으로 덮어쓰지 않음.

## 검증 및 남은 확인

- 위치를 Y=0으로 고치는 시험 창: NOSENDCHANGING 제외 시 실패, 포함 시 원점 보존 통과.
- 화면 밖 시험 창: 확장 프레임 왕복, 다른 스타일 보존, 상하좌우 이동, Y=0 도달, 크기 유지, OFF 이후 이동 중지, 복구 실패 확인 및 재시도 검증.
- 실제 메이플 성공은 별도 확인 필요. 시험 창 결과로 해결 완료 선언 금지.
- 실행은 `cargo run`만 사용. 진단 인자·게임 자동 입력·자동 앱 실행 금지. 버전/릴리즈 유지.

참고 소스는 `.reference/`에 보관하며 Git/빌드 제외.
