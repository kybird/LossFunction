---
title: KIS WebSocket 실시간 클라이언트 구현
status: done
ordinal: 7000
created: 2026-09-14
depends_on: ["Broker abstraction 인터페이스 정의"]
---

## Goal
<!-- kanban:goal:begin -->
실시간 체결/시세 수신을 위한 KIS WebSocket 클라이언트를 재연결과 subscription replay 지원으로 구현한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 연결 끊김 후 자동 재연결이 테스트로 검증된다
- [x] #2 재연결 시 desired subscription 상태가 replay되어 시세가 수신 재개된다
- [x] #3 수신 데이터가 파싱되어 도메인 이벤트로 변환된다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-14T17:33-07:00 — 재연결+subscription replay 클라이언트 구현: desired 상태 도메인 보관 후 재연결마다 전량 재구독, H0STCNT0(46컬럼) Quote 파싱(KST), PINGPONG echo, approval key(secretkey 필드) 발급. 검증: pytest 54 passed(스크립트 연결로 재연결/replay/시세재개/PINGPONG 전 경로), ruff 통과. 커밋 ff1e528
