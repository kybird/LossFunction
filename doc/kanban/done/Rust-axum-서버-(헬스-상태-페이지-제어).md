---
title: Rust axum 서버 (헬스/상태 페이지/제어)
status: done
ordinal: 19000
created: 2026-09-15
depends_on: ["Rust 저장 계층 (SQLite)","Rust 리스크 계층","Rust 오케스트레이터 및 데모 루프"]
---

## Goal
<!-- kanban:goal:begin -->
/healthz(JSON), /(상태 페이지: escape 포함), POST /control/kill-switch(audit 기록)를 구현한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 서브프로세스/통합 테스트로 토글→health→배지→audit→해제가 검증된다
- [x] #2 DB 값 HTML 이스케이프가 단정된다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-15T01:18-07:00 — 라우트(/, /healthz, POST /control/kill-switch: audit 기록)+이스케이프된 상태 페이지+바이너리 진입점(설정 게이트, 데모 루프). Repository::open 상위 디렉터리 생성 버그를 실스모크로 발견·수정. 검증: cargo test 61 passed(oneshot 제어 시나리오)+실측 스모크(헬스/토글/데모 주문 렌더), clippy/fmt 클린
