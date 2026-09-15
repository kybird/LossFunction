---
title: PostgreSQL 스키마 및 저장 계층 구현
status: done
ordinal: 8000
created: 2026-09-14
depends_on: ["시스템 아키텍처 설계 문서화"]
---

## Goal
<!-- kanban:goal:begin -->
시세/캔들/주문/실행/포트폴리오/감사(audit) 데이터를 저장하는 PostgreSQL 스키마와 저장 계층을 구현한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 주요 테이블 마이그레이션이 idempotent하게 적용된다
- [x] #2 저장 계층 CRUD가 통합 테스트로 검증된다
- [x] #3 모든 상태 변경이 audit 로그로 기록된다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-14T17:55-07:00 — asyncpg 저장 계층 구현: 멱등 번호 마이그레이션(quotes/candles/orders/fills/positions/audit_log), 쓰기 트랜잭션 내 audit 이벤트, fills 자연키 멱등. 검증: 실제 PG16 대상 통합테스트 3 passed(이중 마이그레이션 no-op, 주문 수명주기 audit, 견적 삽입), 전체 57 passed. 로컬 PG는 conda 프로젝트 격리+최소 PATH 기동(DLL 충돌 해결, Case 7). 커밋 afb6004
