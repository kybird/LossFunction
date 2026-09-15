---
title: Rust 배포 (Docker/스크립트)
status: done
ordinal: 9000
created: 2026-09-14
depends_on: ["Rust axum 서버 (헬스/상태 페이지/제어)"]
---

## Goal
<!-- kanban:goal:begin -->
multi-stage 빌드(정적 바이너리) Dockerfile과 기존 PowerShell 배포 스크립트를 Rust에 맞게 갱신해 피닉스에 배포 실증한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 컨테이너가 paper+데모로 기동해 헬스가 확인되고 메모리가 Python 대비 감소함을 측정한다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-15T11:54-07:00 — multi-stage Dockerfile(dep-캐시 레이어+distroless nonroot, --healthcheck 자가 probe)+compose(./rust 빌드, mem 80m)+deploy.ps1 rust 트리 패키징. 피닉스 실증: 빌드 완료, healthy, 데모 페이지 렌더, 상주 메모리 37.56MiB(Python 대비 ~1/3), OOM 0
