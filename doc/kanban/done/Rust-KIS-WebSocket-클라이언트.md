---
title: Rust KIS WebSocket 클라이언트
status: done
ordinal: 17000
created: 2026-09-15
depends_on: ["Rust Broker 트레이트 및 MockBroker"]
---

## Goal
<!-- kanban:goal:begin -->
구독 메시지·H0STCNT0(46컬럼) 파싱·PINGPONG echo·재연결 시 구독 replay를 구현한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 주입 연결 팩토리로 재연결+replay 후 시세 재개가 테스트로 검증된다
- [x] #2 프레임 파싱 단일/복수 레코드와 거부 케이스가 단정된다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-15T10:46-07:00 — H0STCNT0 46컬럼 파싱(KST→UTC)+공식 구독 메시지+PINGPONG echo+approval key(secretkey) 발급+주입 팩토리로 재연결 시 desired 구독 replay·시세 재개 단정(스크립트 더블 FIFO 교훈). 검증: cargo test 95 passed×3연속, clippy/fmt 클린
