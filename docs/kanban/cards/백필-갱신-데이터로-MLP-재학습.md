---
title: 백필 갱신 데이터로 MLP 재학습
status: todo
ordinal: 27000
created: 2026-11-20
milestone: 무소속
---

## Goal
<!-- kanban:goal:begin -->
일봉이 2026-11-20까지 갱신됨(기존 모델은 09-21 데이터로 학습, dataset_sha256 불일치). scripts/ml/train_mlp.py 재실행으로 ONNX 3종·meta.json·regression_fixture.json 갱신 — 절차·검증은 2026-11-20 완료 카드와 동일
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [ ] #1 3종목 재학습 후 meta.json data_end=2026-11-20 / 패리티 ≤1e-5 / 재실행 바이트 동일 / walk-forward 정확도·p값 갱신 기록
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
