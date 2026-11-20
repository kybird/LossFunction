---
title: MLP 학습·ONNX 내보내기
status: todo
ordinal: 24000
created: 2026-11-20
milestone: MLP ort 추론 경로
---

## Goal
<!-- kanban:goal:begin -->
reference ml/pipeline.py의 sklearn MLP(MLPClassifier+StandardScaler, 1/5/20 룩백 인과 피처)를 확보된 일봉 5년 실데이터로 학습해 skl2onnx로 ONNX를 내보낸다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [ ] #1 skl2onnx로 내보낸 .onnx가 생성되고 onnxruntime으로 로드된다
- [ ] #2 onnxruntime 출력이 sklearn 추론 출력과 허용오차 내 일치(결정론 시드 고정)
- [ ] #3 학습 분할은 reference의 walk-forward·embargo 절차를 따른다
<!-- kanban:ac:end -->

## Plan

## Notes
- 2026-11-20T13:19+09:00 — 지시서 — 무엇을 할 것: 확보된 일봉(candles 테이블, SQLite)에서 1/5/20 룩백 인과 피처를 뽑아 sklearn MLPClassifier+StandardScaler 학습 후 skl2onnx로 내보낸다(reference ml/pipeline.py를 참고하되 복사해서 새 위치에). 무엇을 건드리지 말 것: rust/ 활성 코드(이 카드는 Python 학습·내보내기만), reference/ 수정 금지. 어디서 틀리기 쉬운가: 피처에 미래 정보(look-ahead) 유입 — 당일 확정 봉만 사용; sklearn↔skl2onnx 버전 호환(미지원 오퍼레이터 시 initial_types/ml 옵션 조정); 학습은 오너 머신 1회성이며 운영 의존 아님

## Handoff

## Result
