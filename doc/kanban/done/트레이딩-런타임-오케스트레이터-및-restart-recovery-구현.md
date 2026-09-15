---
title: 트레이딩 런타임 오케스트레이터 및 restart recovery 구현
status: done
ordinal: 15000
created: 2026-09-14
depends_on: ["Order reconciliation 및 duplicate order 방지 구현","Risk management 계층 구현","Strategy 인터페이스 및 deterministic decision layer 구현","KIS WebSocket 실시간 클라이언트 구현"]
---

## Goal
<!-- kanban:goal:begin -->
시세 수신 → 전략 → 리스크 → 주문 → 조정(reconciliation) 흐름을 묶는 메인 런타임과 크래시 후 복구 절차를 구현한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 이벤트 흐름이 종단 간 통합 테스트(mock broker)로 검증된다
- [x] #2 프로세스 재시작 후 상태 복구와 미결제 주문 처리가 테스트로 검증된다
- [x] #3 복구 절차가 문서화되어 있다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-14T18:22-07:00 — TradingRuntime: 견적→결정→리스크→게이트웨이→실행동기화→포트폴리오 종단 간 흐름 + recover()(미결제 UNKNOWN 등록→reconciliation→broker 잔고 재구성). sync와 reconcile 분리, 저장소 비의존 콜백. docs/recovery.md로 절차 문서화. MockBroker 지정가 크로스 체결 확장. 검증: pytest 133 passed(종단간/재시작 2시나리오/재제출 차단), ruff 통과. 커밋 baa55f7
