---
status: active
version_context: "lossfunction 테스트 더블 관행 (2026-09)"
tags: [testing, pattern, async]
aliases: [test-doubles-by-injection, injectable-connection-factory, noop-sleep-yields]
created: 2026-09-14
confidence: 5
---

# 주입으로 만드는 시험성 (Injection for Testability)

clock/sleep/HTTP transport/연결 팩토리를 주입받게 설계해, 네트워크·시계
없이 전 경로를 테스트한다.

## The Rule

- **transport 주입**: httpx.MockTransport로 실제 endpoint 형상(헤더/바디/
  페이지네이션/재시도)을 계약 테스트 — 실서버 불필요.
- **연결 팩토리 주입**: WebSocket 클라이언트는 ConnectionFactory를 받아
  스크립트형 가짜 연결로 재연결/재구독/PINGPONG 전 경로 검증. 실구현은
  websockets 얇은 어댑터.
- **clock/sleep 주입**: 토큰 만료·백오프·중복 억제 창은 주입 시계로 경계
  양쪽을 결정적으로 테스트.
- **noop sleep은 양보하라**: 주입 sleep이 즉시 반환만 하면(`return None`)
  실패 경로의 무한 루프가 이벤트 루프를 얼린다 — `await asyncio.sleep(0)`
  이라도 제어를 양보하게 만들 것(실측: 48/46 컬럼 불일치 → 파싱 실패 →
  재연결 무한루프 → 테스트 영구 교착).
- **가짜는 진짜 규칙을 따라라**: MockBroker는 지정가 호가 크로스
  체결(매수: 시장가 ≤ 지정가, 체결가=지정가)까지 흉내 — 시장가만 채결하는
  mock으로는 지정가 전략의 종단간 테스트가 불가능하다.

## Why it works

주입 지점이 곧 시험 지점이다. 외부 의존(시계·네트워크·연결)을 경계로
밀어내면 나머지는 순수 함수처럼 결정적으로 검증된다.

## Trade-offs

- 생성자 파라미터 증가 — 팩토리(build_broker 등)로 조립 지점을 하나로
  모아 상쇄.

## Anti-Pattern

- 양보 없는 noop sleep. 실서버 없이는 못 하는 테스트로 치부하기.
- 실제 API를 호출하는 "통합" 테스트를 유닛이라 부르기.

## Related

[[testing-discipline]], [[kis-api]]

## Grounding (References)

- doc/raw/2026-09-14.md Case 4/5 (transport·clock·sleep 주입, `hash:4228a47`, `hash:061eb5a`)
- doc/raw/2026-09-14.md Case 6 (연결 팩토리+교착 교훈, `hash:ff1e528`)
- doc/raw/2026-09-14.md Case 11 (ScriptedBroker, `hash:2d8d4bd`)
- doc/raw/2026-09-14.md Case 14 (MockBroker 크로스 체결, `hash:baa55f7`)
- doc/raw/2026-09-14.md Case 19 (알림 clock 주입, `hash:4a2bd72`)
