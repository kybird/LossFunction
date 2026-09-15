# 배포 및 운영 절차 (OCI, 24/7)

구성 요소: `Dockerfile`(런타임 이미지), `docker-compose.yml`(단일
runtime 서비스, SQLite 데이터 볼륨). 재시작 정책은 `restart: unless-stopped` — 크래시 시 컨테이너
런타임이 자동으로 재기동하고, 런타임의 `recover()` 절차(docs/recovery.md)가
미결제 주문과 포트폴리오를 복구한다.

## 1. OCI 준비 (한 번)

1. OCI 콘솔에서 VM(예: VM.Standard.A1.Flex, Ubuntu 22.04)을 생성하고
   SSH 키를 등록한다. 8080(헬스) 인바운드를 보안 목록/NSG에서 허용한다.
2. SSH 접속 후 Docker 설치:
   ```bash
   sudo apt-get update && sudo apt-get install -y docker.io docker-compose-v2
   sudo usermod -aG docker "$USER"  # 재로그인 필요
   ```

## 2. 배포

```bash
git clone https://github.com/<owner>/LossFunction.git
cd LossFunction

# 자격증명은 호스트 env 파일로만 주입 (.env 는 gitignored)
cp .env.example .env   # KIS_* 등 기입 (paper 모드면 모의 키)

docker compose up -d --build
docker compose ps       # runtime 이 healthy 상태인지 확인
curl http://127.0.0.1:8080/healthz
# {"status": "ok", "trading_mode": "paper", "broker": "MockBroker", ...}
```

## 3. 모드 전환

- 기본: `TRADING_MODE=paper` → 메모리 브로커(네트워크 주문 0).
- KIS 모의투자 연동 paper: `.env`에 `PAPER_BACKEND=kis` + 모의 appkey/secret.
- live: `TRADING_MODE=live`, `LIVE_TRADING_CONFIRMED=true`,
  `KIS_ENVIRONMENT=real` + 실전 키. **셋 중 하나라도 빠지면 프로세스가 설정
  로드에서 거부되어 기동하지 않는다** — 이는 의도된 실패다.

## 4. 운영

- 헬스: `GET /healthz`(모드/브로커/업타임). 컨테이너 HEALTHCHECK가 30초마다
  검사, 3회 연속 실패 시 unhealthy 표시.
- 크래시: `restart: unless-stopped`로 자동 재기동. 재기동 후 복구 절차는
  docs/recovery.md.
- 로그: `docker compose logs -f runtime`.
- 업그레이드: `git pull && docker compose up -d --build`.
- 백업: SQLite 볼륨(`lossfunction-data`) 안의 단일 파일 — 주문/체결/감사
  이력의 원본. `docker cp lossfunction:/data/lossfunction.db backup.db`.

## 5. 로컬 검증

```bash
pip install -e ".[dev]"
python -m lossfunction.runtime.cli   # HEALTH_PORT=8080 기본
curl http://127.0.0.1:8080/healthz
```

`tests/test_runtime_cli.py`가 진입점을 실제 서브프로세스로 띄워 /healthz
응답과 종료를 검증한다. Docker 이미지 자체의 기동 검증은 Docker가 있는
환경(CI 또는 OCI 호스트)에서 `docker compose up` 후 동일 헬스 체크로
수행한다.
