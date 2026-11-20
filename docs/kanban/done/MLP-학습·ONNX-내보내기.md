---
title: MLP 학습·ONNX 내보내기
status: done
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
- [x] #1 skl2onnx로 내보낸 .onnx가 생성되고 onnxruntime으로 로드된다
- [x] #2 onnxruntime 출력이 sklearn 추론 출력과 허용오차 내 일치(결정론 시드 고정)
- [x] #3 학습 분할은 reference의 walk-forward·embargo 절차를 따른다
<!-- kanban:ac:end -->

## Plan

## Notes
- 2026-11-20T13:19+09:00 — 지시서 — 무엇을 할 것: 확보된 일봉(candles 테이블, SQLite)에서 1/5/20 룩백 인과 피처를 뽑아 sklearn MLPClassifier+StandardScaler 학습 후 skl2onnx로 내보낸다(reference ml/pipeline.py를 참고하되 복사해서 새 위치에). 무엇을 건드리지 말 것: rust/ 활성 코드(이 카드는 Python 학습·내보내기만), reference/ 수정 금지. 어디서 틀리기 쉬운가: 피처에 미래 정보(look-ahead) 유입 — 당일 확정 봉만 사용; sklearn↔skl2onnx 버전 호환(미지원 오퍼레이터 시 initial_types/ml 옵션 조정); 학습은 오너 머신 1회성이며 운영 의존 아님
- 2026-11-20T13:37+09:00 — 디자인 결정 2026-11-20: 종목별 모델 3개 — 005930·035420·069500 각각 학습해 ONNX 3파일 산출. 근거: 피처가 로그수익률 3개뿐이라 모델 내용이 종목 고유 자기상관 구조인데 그게 종목마다 다름(069500은 지수 ETF로 단일종목과 이질). 종목당 1,223행·파라미터 약 80개로 표본 부족 감당 가능. 재검토 경계: 종목 수십 개 이상으로 늘면 풀링+종목 피처 재설계. 풀링 vs 개별 최종 판정은 walk-forward 검증 점수로(동전 던지기 대비 유의성도 종목별 확인)

## Handoff

## Result
- 2026-11-20T13:58+09:00 — 완료 — AC 3/3 충족. 산출: models/{005930,035420,069500}.onnx(각 1,591B) + models/meta.json(피처 순서 log_return_1/5/20, 시드 7, 분할 구간, 라이브러리 버전) + models/regression_fixture.json(RAW 27행→onnxruntime 기대출력, 허용오차 1e-5) + scripts/ml/train_mlp.py(reference 절차 복사: 시간순 분할+embargo, train-only StandardScaler, MLPClassifier(16,), max_iter=300, random_state=7; StandardScaler는 Pipeline으로 ONNX 그래프에 포함 — Rust는 원 피처 투입). 검증: (1) onnxruntime 3파일 로드 성공 (2) sklearn predict_proba 대비 최대절대차 1.37e-07/8.56e-08/1.06e-07 — 허용오차 1e-05 내, 라벨 불일치 0, 재실행 바이트 동일(SHA-256) (3) 분할 n_train=900/n_test=301, train_end=2025-06-26, test_start=2025-06-30. walk-forward 검증 정확도(양측 이항검정 p vs 동전): 005930 0.4684(p=0.2995, 무의미) · 035420 0.5415(p=0.1665, 무의미) · 069500 0.5814(p=0.0056, 5% 유의) — 파이프라인 표준화가 1차 목표였고 예측력은 2차로 카드 완성에 지장 없음. 데이터 주의: 오너 복사본 DB — 일봉 2026-09-21까지(금고 locked로 backfill 불가, ~2개월 공백). 버전: sklearn 1.9.1 / skl2onnx 1.20.0 / onnxruntime 1.32.0 / numpy 2.5.5 / Python 3.13.11. 근거: docs/raw/2026-11-20.md Case 2, hash:6b13f94·hash:0496ab2
