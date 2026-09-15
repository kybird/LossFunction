---
status: active
version_context: "lossfunction 테스트 관행 (2026-09)"
tags: [testing, pattern]
aliases: [verify-by-install, exhaustive-pair-testing, perturbation-causality-test, docs-as-tests]
created: 2026-09-14
confidence: 5
---

# 테스트 규율 (Testing Discipline)

이 프로젝트에서 검증을 의미 있게 만드는 패턴들. 공통 원리: **커버리지
논쟁을 끝내는 방식은 부분 열거가 아니라 전체 곱집합 검사다.**

## The Rule

1. **verify-by-install**: 패키지는 venv에 `pip install -e` 후 import로
   검증. flat layout의 미설치 import 통과는 아무것도 증명하지 못한다.
2. **exhaustive-pair-testing**: 상태머신처럼 유한 도메인이면 전 (from,to)
   쌍을 완전 탐사 — 테이블 내 전부 통과, 밖 전부 거부. 부분 열거 대신
   전체 곱집합.
3. **perturbation-causality-test**: 인과성은 "마지막 입력을 치환해도 이전
   출력이 불변"으로 증명(MLP feature, lookahead 가드). 규칙 문서 대신
   인터페이스로 차단(BacktestFeed.bar_at 커서 밖 = LookaheadError).
4. **subprocess-entrypoint-tests**: 진입점은 실제 서브프로세스로 기동해
   HTTP 응답 검증 — 유닛 모킹은 부팅 순서(설정→DB 마이그레이션→서버)를
   증명 못 한다.
5. **docs-as-tests**: README 필수 섹션/명령/스크립트 규칙을 테스트로 고정
   — 문서가 코드 변화에 뒤처지면 빌드가 실패.
6. **동일 입력 2회 == 구조적 동등**: 결정론(전략/백테스트/학습)은 객체
   동등성으로 단정.

## Why it works

각 패턴이 "테스트가 통과했다"의 의미를 좁히고 명확히 만든다. 특히 2~3번은
증명 구조 자체를 테스트에 넣어, 리뷰 논쟁보다 기계적 판정을 남긴다.

## Trade-offs

- 전체 탐사/서브프로세스 테스트는 도메인이 커지면 느려진다 — 유한 도메인과
  진입점에만 적용.

## Anti-Pattern

- "읽어보니 되는 것 같다"식 AC 체크. 커버리지 % 숫자 놀이.
- 결정론을 문서로만 선언.

## Related

[[trading-core-determinism]], [[injection-for-testability]]

## Grounding (References)

- doc/raw/2026-09-14.md Case 1 (verify-by-install, `hash:a79826d`)
- doc/raw/2026-09-14.md Case 10 (전 쌍 탐사, `hash:a2470ac`)
- doc/raw/2026-09-14.md Case 15 (lookahead 인터페이스 차단, `hash:f7c0db4`)
- doc/raw/2026-09-14.md Case 16 (섭동 인과성+embargo, `hash:8b31303`)
- doc/raw/2026-09-14.md Case 18 (서브프로세스 진입점, `hash:9d0a6d3`)
- doc/raw/2026-09-14.md Case 20 (docs-as-tests, `hash:a43514c`)
