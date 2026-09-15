# 자격증명 획득 및 설정 가이드

LossFunction은 기본(paper + memory)이면 **아무 자격증명 없이** 동작합니다.
아래는 KIS 연동 또는 GLM 분석을 켤 때 필요한 것들입니다.

> **공통 규칙**: 자격증명은 `.env` 파일(또는 배포 환경의 env)에만 넣습니다.
> `.env`/`.env.*`는 gitignored, `.env.example`만 저장소에 추적됩니다.
> 실전 키를 커밋한 적이 발견되면 **키를 폐기(재발급)하는 것이 원칙**입니다.

## 1. KIS Open API (실전)

1. **API 신청**: 한국투자증권 [Open API 포털](https://apiportal.koreainvestment.com)
   에서 회원가입 후 "Open API 신청"(개인, 투자목적) — 이후 증권계좌 개설 필요.
2. **AppKey / AppSecret 발급**: 포털의 "앱키 발급" 메뉴에서 발급. 서비스는
   "국내주식" 선택(해외주식은 현재 미지원).
3. **계좌번호**: 8자리-2자리 형식(예: `12345678-01`). HTS/MTS에서 확인.
4. **`.env` 설정**:
   ```ini
   TRADING_MODE=live
   LIVE_TRADING_CONFIRMED=true      # 이중 확인 — 없으면 기동 거부됨
   KIS_ENVIRONMENT=real
   KIS_APP_KEY=<실전 AppKey>
   KIS_APP_SECRET=<실전 AppSecret>
   KIS_ACCOUNT_NUMBER=12345678-01
   ```
   세 값 중 하나라도 어긋나면 프로세스가 기동 단계에서 거부합니다(의도된
   실패 — 자동으로 paper로 떨어지지 않음).

## 2. KIS 모의투자 (paper, 권장 시작점)

**모의투자는 실전과 AppKey/AppSecret이 별도입니다.** 실전 키를 모의
도메인에 쓸 수 없고, 그 반대도 마찬가지입니다.

1. 위와 동일하게 포털에서 신청하되 **"모의투자" 서비스**로 앱을 추가
   발급(모의용 AppKey/AppSecret).
2. 모의투자 계좌는 포털/MTS 모의투자 메뉴에서 개설(역시 8-2 형식).
3. **`.env` 설정**:
   ```ini
   TRADING_MODE=paper
   PAPER_BACKEND=kis             # memory(기본, 네트워크 0) → kis 로 변경
   KIS_ENVIRONMENT=mock
   KIS_APP_KEY=<모의 AppKey>
   KIS_APP_SECRET=<모의 AppSecret>
   KIS_ACCOUNT_NUMBER=<모의 계좌번호>
   ```

## 3. GLM 분석 (선택)

1. [Zhipu 개방플랫폼](https://open.bigmodel.cn) 가입 후 API 키 발급.
2. 현재 코드는 OpenAI 호환 `/chat/completions` 엔드포인트 사용(기본
   `https://open.bigmodel.cn/api/paas/v4`, 모델 `glm-4-flash`).
3. Rust 런타임은 아직 GLM을 자동 호출하지 않습니다(analysis 크레이트는
   구현 완료). 연동 시 `.env`의 `GLM_API_KEY`로 활성화 예정.

## 4. 아무것도 없이 시작하기 (데모, Rust)

```ini
TRADING_MODE=paper
PAPER_BACKEND=memory
DEMO_LOOP=true                 # 가상 시세로 전략-주문-체결이 흐름
# 실행: cd rust && DEMO_LOOP=true cargo run --release
```

## 참고

- 토큰/접속키는 앱이 자동 발급·갱신하므로 사용자가 다룰 필요 없음.
- 도메인 구분: 실전 `openapi.koreainvestment.com`, 모의
  `openapivts.koreainvestment.com` — 설정(KIS_ENVIRONMENT)으로만 전환.
