---
status: active
version_context: "KIS Open API (공식 샘플 koreainvestment/open-trading-api 기준, 2026-09 검증)"
tags: [kis, api, reference]
aliases: [한국투자증권 API, KIS Open API, kis-openapi]
created: 2026-09-14
confidence: 4
---

# KIS API

한국투자증권 Open API의 실측 스펙 모음. 모든 항목은 공식 샘플 저장소
(examples_llm/) 코드로 검증했다. **필드명/형식은 추측 금지 — 문서화된
형상 그대로, 누락 시 크게 실패(fail-loud)한다.**

## 인증 (REST)

- `POST {base}/oauth2/tokenP` — **JSON body** `{"grant_type":
  "client_credentials", "appkey", "appsecret"}` (form-urlencoded `/oauth2/token`
  도 있으나 공식 샘플은 tokenP).
- 200 응답: `access_token`, `access_token_token_expired`
  (`"%Y-%m-%d %H:%M:%S"`, **KST 월시간, 타임존 마커 없음** → 파싱 시
  `timezone(timedelta(hours=9))` 부여 필수).
- 토큰 유효 ~1일. 6시간 이내 재발급 요청은 서버가 동일 토큰 반환.
- WebSocket 접속키는 별도: `POST /oauth2/Approval`, body 필드명이
  **`secretkey`** (REST의 `appsecret`이 아님!).
- 도메인: 실전 `https://openapi.koreainvestment.com:9443`, 모의
  `https://openapivts.koreainvestment.com:9443`.

## 주문/조회 REST (국내주식)

| 기능 | Endpoint | tr_id (실전/모의) | 비고 |
|---|---|---|---|
| 현금주문 | POST `/uapi/domestic-stock/v1/trading/order-cash` | TTTC0012U·TTTC0011U(매수·매도) / V prefix | body **대문자 키 + 전부 문자열**(CANO, ACNT_PRDT_CD, PDNO, ORD_DVSN, ORD_QTY, ORD_UNPR, EXCG_ID_DVSN_CD…). ORD_DVSN 00=지정가 01=시장가(시장가는 ORD_UNPR="0"). 응답 output.ODNO=주문번호, KRX_FWDG_ORD_ORGNO=취소에 필요 |
| 정정취소 | POST `.../order-rvsecncl` | TTTC0013U / VTTC0013U | 취소: RVSE_CNCL_DVSN_CD="02", QTY_ALL_ORD_YN="Y"(잔량 전부, 수량/단가는 "0") |
| 잔고 | GET `.../inquire-balance` | TTTC8434R / VTTC8434R | 페이지네이션: 응답 헤더 tr_cont M/F인 한 ctx_area_fk100/nk100 반송. 보유수량 0 행 존재 |
| 당일체결 | GET `.../inquire-daily-ccld` | TTTC0081R / VTTC0081R | ODNO 필터. 필드: odno/ord_qty/tot_ccld_qty/tot_ccld_amt/cncl_yn/sll_buy_dvsn_cd(02=매수)/pdno — 커뮤니티 문서 기반, 모의 도메인 실증 pending |
| 현재가 | GET `.../quotations/inquire-price` | FHKST01010100(공통) | FID_COND_MRKT_DIV_CODE=J, 현재가 필드 `stck_prpr` |

공통 헤더: authorization Bearer, appkey, appsecret, tr_id, custtype "P",
tr_cont. 계좌는 8-2 자리("12345678-01").

## WebSocket

- URL: 실전 `ws://ops.koreainvestment.com:21000`, 모의
  `ws://vops.koreainvestment.com:21000`.
- 구독: `{"header": {"approval_key", "tr_type": "1"|"0", "custtype": "P"},
  "body": {"input": {"tr_id", "tr_key"}}}`.
- 데이터 프레임: 파이프 구분 `0|TR_ID|TR_KEY|값1^값2^...` — H0STCNT0
  (실시간체결가 KRX) 컬럼 **46개**. 주요 인덱스: MKSC_SHRN_ISCD=0,
  STCK_CNTG_HOUR=1, STCK_PRPR=2, BSOP_DATE=33.
- Keepalive: 서버가 JSON(header.tr_id=="PINGPONG") 전송 → pong으로 echo.
- 주문 체결통보는 암호화(AES): 실전 H0STCNI0 / **모의 H0STCNI9**.

## 에러 응답

- HTTP 200 + `rt_cd != "0"` = 비즈니스 거부(msg_cd/msg1) — 재시도 금지.
- 401 → 토큰 재발급 1회 후 재시도. 429/5xx/전송 실패 → 백오프 재시도.

## Related

[[order-lifecycle]], [[boundary-conversions]], [[fail-loud]]

## Grounding (References)

- doc/raw/2026-09-14.md Case 4 (인증 스펙, `hash:4228a47`)
- doc/raw/2026-09-14.md Case 5 (REST 스펙, `hash:061eb5a`)
- doc/raw/2026-09-14.md Case 6 (WS 스펙, `hash:ff1e528`)
- doc/raw/2026-09-14.md Case 9 (정정취소 스펙, `hash:8c36798`)
