---
title: Rust MLP/GLM 전략 조사
status: done
ordinal: 10000
created: 2026-09-14
---

## Goal
<!-- kanban:goal:begin -->
Rust ML 스택(linfa/candle/ort 등)과 GLM 클라이언트 이관 방안을 조사해 권고한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 후보별 성숙도/의존성/1G 박스 적합성 비교와 권고가 문서화된다
<!-- kanban:ac:end -->

## Plan

## Notes
- 2026-09-15T11:57-07:00 — GLM 클라이언트는 별도 카드로 구현 완료(Rust GLM 클라이언트). MLP는 Rust ML 스택(linfa/candle/ort) 비교 권고 문서화

## Handoff

## Result
- 2026-09-15T11:57-07:00 — GLM은 구현 완료(별도 카드). MLP: Rust ML 후보 비교 — 소규모 MLP 학습+추론, 결정론 재현, 1G 박스 적합성 기준으로 (1)linfa(가장 가벼움, Rust 순수) (2)candle(HF, GPU 불필요 시 무거움) (3)ort ONNX 런타임(학습은 Python, 추론만 Rust — 추천: Python sklearn 학습→ONNX 내보내기→ort 추론이 1G 박스·결정론에 최적) 권고. 근거 문서화
