---
tags: [index]

# Wiki Index

LossFunction 프로젝트의 구조화된 지식 베이스입니다. `doc/raw/` 로그에서 추출한 핵심 개념과 패턴을 정리했습니다.

---

## Concepts

| 개념 | 설명 | 별칭 |
|------|------|------|
| [[deployment-operations]] | Docker 단일 컨테이너 + SQLite 볼륨 + PowerShell 배포 스크립트. | 배포 운영, paper-live-separation, restart-policy, paper-live 분리 |
| [[kis-api]] | 한국투자증권 Open API의 실측 스펙 모음. | 한국투자증권 API, KIS Open API, kis-openapi |
| [[order-lifecycle]] | 주문의 상태 전이, 제출, 조정(reconciliation)의 전체 규칙. | 주문 수명주기, order-state-machine, reconciliation, idempotency-key, 중복 주문 차단 |
| [[storage-sqlite]] | 단일 파일 SQLite(WAL) 저장소. | 저장 계층, sqlite-storage, scaled-integer-money |
| [[trading-core-determinism]] | 전략·리스크·주문은 순수 함수 기반: **동일 입력에 동일 결정**, 전 판단 재현 | 결정론적 거래 코어, deterministic-strategy, advisory-signals, 자문 신호 |

---

## Patterns

| 패턴 | 설명 | 별칭 |
|------|------|------|
| [[boundary-conversions]] | 시스템 경계(외부 API ↔ 내부 도메인 ↔ 저장소 ↔ 출력)에서만 타입을 바꾸고, | 경계 변환, boundary-type-conversion, escape-at-boundary |
| [[fail-loud]] | 조용한 오답보다 명시적 실패를 택하는 오류 처리 패턴 모음. | status-first-classification, explicit-fallback, fail-boot-on-bad-config |
| [[injection-for-testability]] | clock/sleep/HTTP transport/연결 팩토리를 주입받게 설계해, 네트워크·시계 | test-doubles-by-injection, injectable-connection-factory, noop-sleep-yields |
| [[testing-discipline]] | 이 프로젝트에서 검증을 의미 있게 만드는 패턴들. | verify-by-install, exhaustive-pair-testing, perturbation-causality-test, docs-as-tests |

---

## Anti-Patterns

| 안티패턴 | 설명 | 별칭 |
|------|------|------|
| [[windows-polluted-service-env]] | Windows에서 외부 프로세스(서비스/빌드)를 현재 셸 환경 그대로 기동하면, | dll-search-order, polluted-path-service-start, msys-path-conversion |

---

## Answers

| 답변 | 설명 | 별칭 |
|------|------|------|

---

## Statistics

- Total concepts: 5
- Total patterns: 4
- Total anti-patterns: 1
- Total answers: 0
- Last updated: 2026-09-14
