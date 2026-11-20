---
title: 저장 계층 SQLite 전환
status: done
ordinal: 23000
created: 2026-09-14
---

## Goal
<!-- kanban:goal:begin -->
저장 계층을 asyncpg/PostgreSQL에서 aiosqlite/SQLite(WAL)로 전환해 단일 파일 운영·저메모리 배치에 맞춘다. 화폐는 1e-4원 정수 스케일으로 정확 저장
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 Repository가 SQLite 파일로 마이그레이션/CRUD/audit을 수행하고 기존 테스트 시맨틱이 유지된다
- [x] #2 화폐 값이 Decimal↔스케일 정수로 왕복 변환되며 오차가 없음이 테스트로 검증된다
- [x] #3 PostgreSQL 의존(asyncpg, dev_postgres.sh, compose postgres 서비스, DATABASE_URL)이 제거되고 통합 테스트가 외부 서버 없이 항상 실행된다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-14T20:06-07:00 — aiosqlite/WAL 저장소로 전환: 화폐 INTEGER 1e-4원 스케일 왕복 정확(테스트 단정), BEGIN IMMEDIATE+Lock 쓰기 직렬화, 타임스탬프 앱 공급 UTC ISO. asyncpg/dev_postgres.sh/compose postgres/DATABASE_URL 제거, 통합 테스트가 tmp 파일로 어디서나 실행. 검증: pytest 177 passed(동시 쓰기 20건 포함), ruff 통과. 커밋 46f3713
