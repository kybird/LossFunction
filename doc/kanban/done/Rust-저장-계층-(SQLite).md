---
title: Rust 저장 계층 (SQLite)
status: done
ordinal: 3000
created: 2026-09-14
depends_on: ["Rust 도메인 모델 (Order/Portfolio)"]
---

## Goal
<!-- kanban:goal:begin -->
sqlx로 동일 SQLite 스키마(WAL, 1e-4원 정수 화폐, 앱 공급 UTC ISO)와 Repository(CRUD+쓰기 트랜잭션 내 audit)를 구현한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 마이그레이션 멱등과 주문 수명주기+audit, 화폐 왕복 정확성 테스트가 통과한다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-15T00:29-07:00 — sqlx/WAL 저장소: 동일 스키마(1e-4원 정수·앱 공급 ISO·쓰기 트랜잭션 내 audit), 번호 멱등 마이그레이션, fills 자연키 멱등, quotes/orders/positions/audit 조회. 검증: cargo test 21 passed(멱등·수명주기 audit·화폐 왕복), clippy/fmt 클린
