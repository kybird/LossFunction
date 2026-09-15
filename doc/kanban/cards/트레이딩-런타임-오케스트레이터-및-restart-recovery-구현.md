---
title: 트레이딩 런타임 오케스트레이터 및 restart recovery 구현
status: doing
ordinal: 15000
created: 2026-09-14
depends_on: ["Order reconciliation 및 duplicate order 방지 구현","Risk management 계층 구현","Strategy 인터페이스 및 deterministic decision layer 구현","KIS WebSocket 실시간 클라이언트 구현"]
claimed_by: zcode-main
claimed_at: 2026-09-14T18:16-07:00
---

## Goal
<!-- kanban:goal:begin -->
시세 수신 → 전략 → 리스크 → 주문 → 조정(reconciliation) 흐름을 묶는 메인 런타임과 크래시 후 복구 절차를 구현한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [ ] #1 이벤트 흐름이 종단 간 통합 테스트(mock broker)로 검증된다
- [ ] #2 프로세스 재시작 후 상태 복구와 미결제 주문 처리가 테스트로 검증된다
- [ ] #3 복구 절차가 문서화되어 있다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
