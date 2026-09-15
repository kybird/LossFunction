---
title: MLP 학습 및 추론 파이프라인 구현
status: done
ordinal: 17000
created: 2026-09-14
depends_on: ["PostgreSQL 스키마 및 저장 계층 구현"]
---

## Goal
<!-- kanban:goal:begin -->
feature 생성, time-series split 학습, 검증, 추론, 모델 버전 관리를 갖춘 MLP 파이프라인을 구현한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 feature 파이프라인이 time-series split에서 leakage 없이 동작함이 테스트로 검증된다
- [x] #2 학습된 모델이 버전과 메타데이터로 저장/로드된다
- [x] #3 추론 결과가 strategy decision layer 입력 스키마와 일치한다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-14T18:30-07:00 — 인과 feature(섭동 테스트)+시간순 분할+embargo 1행+학습분할 전용 스케일러로 leakage 삼중 방어, ModelBundle 버전/메타데이터 저장-로드 왕복(재현성 단정), ModelSignal이 MarketSnapshot.signals 자문 스키마로 통합. sklearn은 [ml] extra. 검증: pytest 147 passed, ruff 통과. 커밋 8b31303
