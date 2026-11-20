---
title: Paper 데모 루프
status: done
ordinal: 3000
created: 2026-09-14
depends_on: ["상태 페이지 최소 제어 기능"]
---

## Goal
<!-- kanban:goal:begin -->
paper+memory 환경에서 가상 시세로 전략-주문-체결-저장까지 흐르는 데모 루프를 진입점에 탑재해 상태 페이지에 실시간 데이터가 쌓이게 한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 DEMO_LOOP=true && paper+memory일 때 결정 사이클이 돌고 SQLite에 견적/주문/체결/포지션이 기록된다
- [x] #2 kill switch on 시 데모 주문이 차단됨이 확인된다
- [x] #3 루프가 결정론적 시드로 재현 가능하다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-14T22:54-07:00 — DemoLoop: 시드 랜덤워크 시세가 실제 파이프라인 통과(결정→리스크→게이트웨이→mock 체결)하며 SQLite에 견적/주문/체결/포지션 기록. DEMO_LOOP=true+MockBroker 가드, kill switch 차단 단정, 시드 재현 단정. 피닉스 배포 후 페이지에 실시간 주문 확인. 검증: 190 passed. 커밋 ee70108
