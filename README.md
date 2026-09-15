# LossFunction

오픈소스 자동투자 시스템 — 한국투자증권(KIS) Open API 기반, 국내 주식,
OCI에서 24/7 무인 운영.

> **상태**: 활발히 개발 중. 기본 모드는 paper(모의)이며 live 거래는
> 이중 확인 없이는 기동하지 않습니다. 투자에 따른 손실에 대한 책임은
> 사용자에게 있습니다.

## 기능

- **KIS Open API 연동** — REST(주문/잔고/시세/체결조회) + WebSocket(실시간
  체결가, 재연결 시 구독 자동 재생)
- **결정론적 거래 코어** — 전략/리스크/주문은 순수 함수 기반. 동일 입력에
  동일 결정, 모든 판단이 재현 가능
- **주문 수명주기 관리** — 명시적 상태머신, timeout은 `UNKNOWN`으로
  reconciliation 대기(재시도 금지), client id 기반 중복 주문 차단
- **리스크 계층** — 주문 한도/포지션 상한/총 노출/일일 손실 한도/시세 신선도
  검사 + kill switch
- **SQLite 저장** — 단일 파일(WAL)에 주문/체결/포지션/감사(audit) 이력,
  쓰기 트랜잭션에 포함된 감사 기록, 서버 프로세스 불필요
- **백테스팅** — 수수료/증권거래세 반영, look-ahead 접근 차단, 라이브와 동일한
  전략 인터페이스
- **MLP 파이프라인** — 시계열 leakage 방어(인과 feature, embargo, 분할
  스케일링), 버전화된 모델 번들
- **GLM 분석 통합** — 엄격한 스키마 검증, 장애 시 명시적 폴백(거래 지속)
- **운영** — Docker/compose 배포, 헬스 체크, 자동 재시작 + 재시작 복구,
  메트릭/알림(웹훅 옵트인), 경량 상태 페이지(`GET /` — 포지션/주문/체결/
  감사 이력, 프레임워크 없음)

아키텍처 개요와 모듈 경계는 [docs/architecture.md](docs/architecture.md),
재시작 복구 절차는 [docs/recovery.md](docs/recovery.md),
배포/운영은 [docs/deployment.md](docs/deployment.md)를 보세요.

## 빠른 시작 (개발)

```bash
git clone https://github.com/<owner>/LossFunction.git
cd LossFunction

python -m venv .venv
source .venv/bin/activate            # Windows: .venv\Scripts\activate
pip install -e ".[dev]"              # MLP까지: pip install -e ".[dev,ml]"

pytest                               # 전체 테스트 (자격증명/DB 불필요)
```

린트/포맷: `ruff check . && ruff format --check .`

## 설정

설정은 환경변수와 `.env` 파일에서 로드되고 기동 시점에 검증됩니다.

```bash
cp .env.example .env
```

| 변수 | 기본 | 설명 |
|---|---|---|
| `TRADING_MODE` | `paper` | `paper` \| `live` |
| `LIVE_TRADING_CONFIRMED` | `false` | live 모드의 명시적 이중 확인 |
| `KIS_ENVIRONMENT` | `mock` | `mock`(모의투자) \| `real` |
| `PAPER_BACKEND` | `memory` | `memory`(네트워크 없음) \| `kis`(모의 도메인) |
| `KIS_APP_KEY` / `KIS_APP_SECRET` | — | KIS 자격증명(**저장소에 커밋 금지**) |
| `KIS_ACCOUNT_NUMBER` | — | 계좌번호(8-2 형식) |
| `DATABASE_PATH` | `data/lossfunction.db` | SQLite 파일 경로(WAL) |
| `ALERT_WEBHOOK_URL` | — | 알림 웹훅(옵트인) |

**자격증명은 절대 커밋하지 않습니다.** `.env`/`.env.*`는 gitignored,
`.env.example`만 추적됩니다(누출 방지가 테스트로 검증됨).

### paper trading

기본 설정(`paper` + `memory`)은 브로커 없이 메모리에서 주문이 체결되는
완전한 paper 모드입니다 — 자격증명 없이 전체 파이프라인을 실행할 수
있습니다. KIS 모의투자 도메인으로 paper 거래를 하려면 `PAPER_BACKEND=kis`와
모의 appkey/secret을 설정하세요.

## 실행

```bash
python -m lossfunction.runtime.cli        # 헬스(8080) + 상태 페이지
curl http://127.0.0.1:8080/healthz        # JSON 헬스
# 브라우저에서 http://127.0.0.1:8080/     # SQLite 기반 상태 페이지
```

컨테이너 배포(피닉스 VPS 실증 완료):

```powershell
powershell -ExecutionPolicy Bypass -File scripts\deploy\deploy.ps1
```

상세한 배포/운영 절차는 [docs/deployment.md](docs/deployment.md).

## 프로젝트 구조

```
src/lossfunction/
├── domain/       # 순수 도메인 (주문 규칙, 포트폴리오 집계)
├── broker/       # Broker 추상화 + KIS 구현 + 메모리 mock
├── marketdata/   # KIS WebSocket (재구독 replay)
├── execution/    # 상태머신, 주문 게이트웨이, reconciliation
├── risk/         # 사전 검증 + kill switch
├── strategy/     # 결정론적 전략 인터페이스 + 결정 녹화
├── storage/      # SQLite 마이그레이션/저장소/감사
├── backtest/     # 이벤트 시뮬레이션 (비용, look-ahead 차단)
├── ml/           # MLP 파이프라인 ([ml] extra)
├── analysis/     # GLM 통합 (스키마 검증 + 폴백)
├── runtime/      # 오케스트레이터 + 컨테이너 진입점
└── ops/          # 메트릭 + 알림
```

## 기여

이 저장소는 llm-wiki로 지식을 관리합니다 — `doc/`이 프로젝트 지식베이스,
`doc/kanban/`이 작업 보드입니다. 개발 워크플로우는 `AGENTS.md`를 보세요.

## 라이선스

[MIT](LICENSE) © kybird
