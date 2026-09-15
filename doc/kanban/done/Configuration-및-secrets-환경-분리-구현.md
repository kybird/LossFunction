---
title: Configuration 및 secrets 환경 분리 구현
status: done
ordinal: 3000
created: 2026-09-14
depends_on: ["Project scaffold 및 Python 패키지 기반 구축"]
---

## Goal
<!-- kanban:goal:begin -->
환경변수/설정 파일 기반 configuration 시스템을 만들고 secret을 저장소에서 완전히 분리하며 paper/live 환경을 구분한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 설정이 환경변수와 설정 파일에서 로드되고 검증된다
- [x] #2 secret 패턴이 포함된 파일이 .gitignore로 차단되고 lint/테스트로 검증된다
- [x] #3 paper/live 모드가 설정 수준에서 구분되고 기본값이 paper다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-14T17:03-07:00 — pydantic-settings 기반 Settings 구현: env+.env 로드/검증, 기본 paper, live는 live_trading_confirmed 없으면 거부, KIS 자격증명 SecretStr 마스킹. 검증: pytest 10 passed(로드/검증/gitignore 가드), git check-ignore 확인, ruff 통과. 커밋 8345787
