//! 거래량순위(FHPST01710000) — liquidity screener over the KIS ranking API.
//!
//! Verified against the official sample (examples_llm/domestic_stock/
//! volume_rank): GET /uapi/domestic-stock/v1/quotations/volume-rank with
//! fid_cond_scr_div_code=20171. The request itself carries the Korean
//! risk filters (추측 금지 원칙: `fid_trgt_exls_cls_code` 비트열이 서버에서
//! 위험 종목을 제외한다 — 아래 EXLS 상수 주석 참고). Response row field
//! names are pending live verification like earlier endpoints; the parser
//! accepts the documented aliases and fails loud with the row it got.

use serde_json::Value;

use crate::kis::rest::KisRestClient;

const PATH: &str = "/uapi/domestic-stock/v1/quotations/volume-rank";
const TR_ID: &str = "FHPST01710000";

/// 10자리 제외 플래그(서버측 위험 필터), 순서대로:
/// 투자위험/경고/주의=1, 관리종목=1, 정리매매=1, 불성실공시=1, 우선주=1,
/// 거래정지=1, ETF=0(유지 — KODEX 등), ETN=1, 신용주문불가=0, SPAC=1
const EXLS_RISK_OFF: &str = "1111110101";

/// 가격대 하한/상한(잡주 필터): 2,000원 ~ 1,000,000원.
const PRICE_MIN: &str = "2000";
const PRICE_MAX: &str = "1000000";

#[derive(Debug, Clone, PartialEq)]
pub struct RankedSymbol {
    pub code: String,
    pub name: String,
    /// 누적 거래대금(원).
    pub traded_value: i64,
}

/// 거래금액순 상위 조회 — 위험 종목은 요청 단계에서 제외된다.
pub async fn volume_rank_top(
    rest: &KisRestClient,
    top: usize,
) -> Result<Vec<RankedSymbol>, String> {
    let query = [
        ("FID_COND_MRKT_DIV_CODE", "J".to_string()),
        ("FID_COND_SCR_DIV_CODE", "20171".to_string()),
        ("FID_INPUT_ISCD", "0000".to_string()),
        ("FID_DIV_CLS_CODE", "0".to_string()),
        ("FID_BLNG_CLS_CODE", "3".to_string()), // 거래금액순
        ("FID_TRGT_CLS_CODE", "111111111".to_string()),
        ("FID_TRGT_EXLS_CLS_CODE", EXLS_RISK_OFF.to_string()),
        ("FID_INPUT_PRICE_1", PRICE_MIN.to_string()),
        ("FID_INPUT_PRICE_2", PRICE_MAX.to_string()),
        ("FID_VOL_CNT", "0".to_string()),
        ("FID_INPUT_DATE_1", String::new()),
    ];
    let (body, _) = rest
        .request(reqwest::Method::GET, PATH, TR_ID, None, Some(&query), None)
        .await
        .map_err(|error| format!("거래금액순위 조회 실패: {error}"))?;

    let rows = body
        .get("output")
        .and_then(Value::as_array)
        .ok_or_else(|| "거래금액순위 응답에 output 배열 없음".to_string())?;

    rows.iter()
        .take(top)
        .map(|row| {
            let field = |names: &[&str]| -> Option<String> {
                names
                    .iter()
                    .find_map(|name| row.get(*name).and_then(Value::as_str))
                    .map(str::to_string)
            };
            let code = field(&["mksc_shrn_iscd", "stck_shrn_iscd"])
                .ok_or_else(|| format!("순위 행에 종목코드 없음(실증 대기 필드명): {row}"))?;
            let name = field(&["hts_kor_isnm"]).unwrap_or_default();
            let traded_value = field(&["acml_tr_pbmn"])
                .and_then(|raw| raw.parse::<i64>().ok())
                .unwrap_or(0);
            Ok(RankedSymbol {
                code,
                name,
                traded_value,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kis::auth::KisAuth;
    use crate::kis::rest::KisRestClient;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn client(base_url: String) -> KisRestClient {
        let auth = KisAuth::new(
            base_url.clone(),
            "appkey-1".into(),
            "appsecret-1".into(),
            reqwest::Client::new(),
        );
        KisRestClient::new(
            auth,
            "12345678-01",
            "real",
            base_url,
            reqwest::Client::new(),
            "paper",
        )
        .unwrap()
    }

    /// The request carries the risk-exclusion flags and the price band; the
    /// parser maps the ranking rows into RankedSymbol.
    #[tokio::test]
    async fn volume_rank_parses_and_filters_by_request() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/tokenP"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "t", "access_token_token_expired": "2099-01-01 10:00:00"
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(PATH))
            .and(query_param("FID_BLNG_CLS_CODE", "3"))
            .and(query_param("FID_TRGT_EXLS_CLS_CODE", EXLS_RISK_OFF))
            .and(query_param("FID_INPUT_PRICE_1", PRICE_MIN))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({ "rt_cd": "0", "output": [
                    {"mksc_shrn_iscd": "005930", "hts_kor_isnm": "삼성전자", "acml_tr_pbmn": "1200000000000"},
                    {"mksc_shrn_iscd": "000660", "hts_kor_isnm": "SK하이닉스", "acml_tr_pbmn": "800000000000"}
                ]}),
            ))
            .mount(&server)
            .await;

        let ranked = volume_rank_top(&client(server.uri()).await, 10)
            .await
            .unwrap();
        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked[0].code, "005930");
        assert_eq!(ranked[0].name, "삼성전자");
        assert_eq!(ranked[0].traded_value, 1_200_000_000_000);
    }

    /// A row without any known code field fails loud with the row attached.
    #[tokio::test]
    async fn unknown_row_shape_fails_loud() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/tokenP"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "t", "access_token_token_expired": "2099-01-01 10:00:00"
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({ "rt_cd": "0", "output": [{"unexpected": "shape"}] }),
            ))
            .mount(&server)
            .await;
        let error = volume_rank_top(&client(server.uri()).await, 10)
            .await
            .unwrap_err();
        assert!(error.contains("실증 대기"), "got: {error}");
    }
}
