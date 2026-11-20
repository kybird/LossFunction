---
title: 백필 갱신 데이터로 MLP 재학습
status: done
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
- [x] #1 3종목 재학습 후 meta.json data_end=2026-11-20 / 패리티 ≤1e-5 / 재실행 바이트 동일 / walk-forward 정확도·p값 갱신 기록
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-11-20T18:37+09:00 — 완료 — 백필로 갱신된 일봉(2026-09-21→2026-11-20, 1,223→1,263봉)으로 재학습. scripts/ml/train_mlp.py 1회 실행으로 전 절차 재현: 분할 n_train=930/n_test=311, 검증 정확도(양측 p) 005930 0.4791(p=0.4963) · 035420 0.5209(p=0.4963) · 069500 0.5659(p=0.0232 — 5% 유의 유지, 0.0056→0.0232 약화: 최신 40영업일 추가의 효과, 예측력 판단은 후속 실험 소관). 패리티 최대차 1.12e-07 ≤ 허용오차 1e-5, 재실행 바이트 동일(SHA-256), fixture 독립 재현 PASS. meta.json 소스 문구를 백필 기준으로 정정(스크립트 수정 포함). 근거: docs/raw/2026-11-20.md Case 8, hash:92d5e87
