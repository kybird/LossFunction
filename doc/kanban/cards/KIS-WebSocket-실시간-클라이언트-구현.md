---
title: KIS WebSocket 실시간 클라이언트 구현
status: doing
ordinal: 7000
created: 2026-09-14
depends_on: ["Broker abstraction 인터페이스 정의"]
claimed_by: zcode-main
claimed_at: 2026-09-14T17:15-07:00
---

## Goal
<!-- kanban:goal:begin -->
실시간 체결/시세 수신을 위한 KIS WebSocket 클라이언트를 재연결과 subscription replay 지원으로 구현한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [ ] #1 연결 끊김 후 자동 재연결이 테스트로 검증된다
- [ ] #2 재연결 시 desired subscription 상태가 replay되어 시세가 수신 재개된다
- [ ] #3 수신 데이터가 파싱되어 도메인 이벤트로 변환된다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
