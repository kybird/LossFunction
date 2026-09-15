---
title: README 및 공개 문서 작성
status: done
ordinal: 21000
created: 2026-09-14
depends_on: ["Project scaffold 및 Python 패키지 기반 구축"]
---

## Goal
<!-- kanban:goal:begin -->
GitHub 공개에 필요한 README, 아키텍처 문서, 설정/실행 가이드를 작성한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 README에 설치/설정/실행/paper trading 가이드가 포함된다
- [x] #2 secret 없이 누구나 repo clone 후 테스트 실행까지 도달하는 절차가 문서화된다
- [x] #3 프로젝트 문서에서 secret이나 개인 정보가 발견되지 않는다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-14T18:42-07:00 — README 작성(기능/빠른시작 clone→pytest/설정 표/paper 모드 안내/실행/구조/기여) + 문서 위생 테스트(필수 섹션, .env.example 자격증명 비어 있음, base64/hex blob·이메일 스캔). 검증: pytest 174 passed(문서 테스트 6종 포함), ruff 통과. 라이선스는 HANDOFF 카드로 인간 판정 이관. 커밋 a43514c
