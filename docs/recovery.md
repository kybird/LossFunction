# 재시작 복구 절차 (Restart Recovery)

런타임 크래시/재배포 후 새 프로세스가 거래를 재개하기까지의 절차.
구현 위치: `src/lossfunction/runtime/orchestrator.py` (`TradingRuntime.recover`).

## 절차

1. **설정 로드** — `trading_mode=live`는 `live_trading_confirmed=true` +
   `kis_environment=real` 없으면 로드 단계에서 거부된다 (설정 이중 확인).
2. **미결제 주문 수집** — 이전 프로세스가 저장한 `client_order_id →
   broker_order_id` 매핑을 저장소(orders 테이블)에서 읽는다.
3. **UNKNOWN 등록 후 reconciliation** — 각 미결제 주문을 상태머신에
   `UNKNOWN`으로 등록하고 broker의 실행 보고(inquire-daily-ccld)로 해소한다:
   - 전량 체결 → `FILLED`
   - 잔량 존재 → `SUBMITTED` / `PARTIALLY_FILLED` (로컬 open 집합에 유지)
   - 잔량 취소 → `CANCELLED`
   - broker에 없음 → `REJECTED` (도달하지 못한 주문)
4. **포트폴리오 재구성** — 로컬 포지션 상태를 버리고 broker 잔고 조회
   (inquire-balance)로 재구성한다. broker가 사실의 원천이다.
5. **시세 재구독** — WebSocket 클라이언트가 desired subscription을 replay
   한다 (연결 상태가 아니라 도메인 상태로 보관되므로 자동).
6. **신규 주문 허용** — 위 1–5가 끝난 뒤에만 결정 사이클이 주문을 만든다.

## 불변식

- reconciliation 완료 전 동일 `client_order_id` 재제출 금지
  (`OrderGateway`가 기계적으로 차단).
- timeout은 `UNKNOWN`이지 실패가 아니다. 재시도는 reconciliation 이후에만.
- kill switch가 켜져 있으면 모든 결정 사이클의 주문이 즉시 차단된다.

## 테스트

`tests/test_runtime.py::test_restart_recovery_*` — MockBroker로 전 절차를
재현한다: 주문 → 채결 → 프로세스 "재시작"(새 런타임) → 미결제 reconciliation →
포트폴리오 재구성.
