---
title: 문서 Rust 중심 갱신
status: done
ordinal: 22000
created: 2026-09-15
depends_on: ["Rust 배포 (Docker/스크립트)"]
---

## Goal
<!-- kanban:goal:begin -->
README/배포/자격증명 문서를 Rust 기준으로 갱신한다(빌드·실행·배포 명령)
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 문서의 명령이 실제 동작함이 검증되고 문서 테스트가 통과한다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-15T12:22-07:00 — README(빠른시작 cargo test/run, 구조 rust/lossfunction, 배포 실증 갱신)·credentials(데모 실행 cargo 명령, GLM 상태 명시)·deployment(distroless 37.6MiB) 갱신. 문서 정책 테스트는 scripts/check_docs.py로 이동, 아카이브 181 테스트 통과(경로 수정 포함). 명령 실동작 검증
