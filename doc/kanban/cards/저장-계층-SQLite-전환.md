---
title: 저장 계층 SQLite 전환
status: doing
ordinal: 23000
created: 2026-09-14
claimed_by: zcode-main
claimed_at: 2026-09-14T19:58-07:00
---

## Goal
<!-- kanban:goal:begin -->
저장 계층을 asyncpg/PostgreSQL에서 aiosqlite/SQLite(WAL)로 전환해 단일 파일 운영·저메모리 배치에 맞춘다. 화폐는 1e-4원 정수 스케일으로 정확 저장
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [ ] #1 Repository가 SQLite 파일로 마이그레이션/CRUD/audit을 수행하고 기존 테스트 시맨틱이 유지된다
- [ ] #2 화폐 값이 Decimal↔스케일 정수로 왕복 변환되며 오차가 없음이 테스트로 검증된다
- [ ] #3 PostgreSQL 의존(asyncpg, dev_postgres.sh, compose postgres 서비스, DATABASE_URL)이 제거되고 통합 테스트가 외부 서버 없이 항상 실행된다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
