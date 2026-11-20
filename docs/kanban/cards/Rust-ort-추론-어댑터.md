---
title: Rust ort 추론 어댑터
status: todo
ordinal: 25000
created: 2026-11-20
depends_on: ["MLP 학습·ONNX 내보내기"]
milestone: MLP ort 추론 경로
---

## Goal
<!-- kanban:goal:begin -->
ort 크레이트로 ONNX MLP를 로드해 Python 의존 없이 추론하고 MLP-GLM 전략에 advisory 신호로 연결한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [ ] #1 고정 입력→고정 출력 회귀 테스트가 Python onnxruntime 결과와 일치한다
- [ ] #2 MLP-GLM 전략 advisory 신호가 ort 경로로 계산된다
- [ ] #3 clippy·fmt 클린, 전체 테스트 통과
<!-- kanban:ac:end -->

## Plan

## Notes
- 2026-11-20T13:19+09:00 — 지시서 — 무엇을 할 것: ort 크레이트로 ONNX 로드·추론 모듈 추가, 고정 입력 회귀 테스트(Python onnxruntime 산출과 허용오차 비교). 무엇을 건드리지 말 것: 기존 전략 6종·상태머신·저장 계층 변경 금지, advisory 신호는 주문 경로에 개입 금지(리스크 게이트·kill switch 그대로). 어디서 틀리기 쉬운가: ort 실행기 다운로드 방식 있음 — 1G 박스·오프라인 빌드 전제에 맞는 로딩 방식 확인; 결정론(스레드 수·시드) 설정 누락 시 Python 결과와 미세 오차; StandardScaler는 ONNX 그래프에 포함되므로 Rust는 원 피처만 투입

## Handoff

## Result
