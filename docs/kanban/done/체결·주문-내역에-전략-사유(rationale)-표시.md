---
title: 체결·주문 내역에 전략 사유(rationale) 표시
status: done
ordinal: 28000
created: 2026-11-21
milestone: 무소속
---

## Goal
<!-- kanban:goal:begin -->
오너 요청(2026-11-21 심야): 최근 체결/주문이 어떤 근거로 발생했는지 UI에서 보여준다. StrategyDecision.rationale을 주문 저장 시점에 orders.rationale로 스냅샷(migration v5)하고 fills 조회를 orders에 LEFT JOIN해 개요/포지션 페이지 체결 표와 주문 내역 표에 사유 열 추가
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 migration v5 적용 후 기존 행 호환(기본 빈 문자열) / 개요·포지션 페이지 체결·주문 표에 사유 열 렌더(HTML 이스케이프) / cargo test 전체 통과 + 서버 재배포 후 데모 체결에 사유 표시 확인
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-11-21T10:41+09:00 — 완료 — 전략 결정 근거가 체결·주문 표에 보인다. 구현: migration v5(orders.rationale 스냅샷, 기존 행 빈값 호환) + create_order(+rationale) + run_decision_cycle이 (Order, rationale) 반환 + demo/sim/realtime 3경로 + fills/orders LEFT JOIN + 개요·포지션 페이지 사유 열(esc 이스케이프). 검증: 169 테스트 통과, 서버 재배포 3초 healthy, 배포 후 신규 체결(01:40 NAVER 매수)에 'buy symbols at/below entry with no existing position' 렌더 확인, 기능 도입 전 체결은 빈 사유(설계대로). 근거: docs/raw/2026-11-21.md Case 2, hash:4c8c59c
