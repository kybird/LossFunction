---
title: Rust 워크스페이스 부트스트랩
status: done
ordinal: 1000
created: 2026-09-14
---

## Goal
<!-- kanban:goal:begin -->
rust/ 하에 Cargo 워크스페이스와 코어 타입(심볼/가격/수량/주문유형), 설정 로더(paper 기본, live 이중 확인 검증)를 세우고 테스트가 통과한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 cargo test가 코어 타입 검증과 설정 규칙(기본 paper, live 거부, real 도메인 강제) 테스트를 통과한다
- [x] #2 크레이트 컴파일 경고가 없다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-15T00:00-07:00 — rust/ Cargo 워크스페이스+lossfunction 크레이트: Symbol/Decimal Price/enum 타입, 설정 로더(paper 기본, live 이중 확인+real 강제, Secret Debug 마스킹), env 테스트 Mutex 직렬화. 검증: cargo test 4 passed, clippy 경고 0. 커밋 9f4ea0f
