# Python 참조 구현 (아카이브)

이 디렉터리는 LossFunction의 **초기 Python 구현**을 참조용으로 보존한다.
현재 운영 구현은 `rust/`(실행 파일 `lossfunction`, 108 테스트)이며 피닉스
VPS에 배포되어 있다.

- 동작 패리티는 `doc/raw/2026-09-15.md` Case 11의 모듈별 대조표로 검증됨
  (전 모듈 동등, MLP 학습은 여기서만 유지 — sklearn→ONNX→ort 경로 권고)
- 실행: `cd reference && pip install -e ".[dev]" && pytest`
- 이 코드는 더 이상 적극 개발되지 않는다(버그 수정도 rust/에서).

배포·운영 문서는 루트 `docs/`가 정본이다.
