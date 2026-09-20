//! wiremock contract tests for the KIS daily-chart MarketDataSource
//! (wiki: kis-api — FHKST03010100 shapes).

use crate::kis::auth::KisAuth;
use crate::kis::chart::KisChartSource;
use crate::kis::rest::KisRestClient;
use crate::marketdata::MarketDataSource;
use crate::types::Symbol;
use chrono::{DateTime, Duration, Utc};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn symbol() -> Symbol {
    Symbol::parse("005930").unwrap()
}

async fn source(base_url: String) -> KisChartSource {
    let auth = KisAuth::new(
        base_url.clone(),
        "appkey-1".into(),
        "appsecret-1".into(),
        reqwest::Client::new(),
    );
    let rest = KisRestClient::new(
        auth,
        "12345678-01",
        "mock",
        base_url,
        reqwest::Client::new(),
        "paper",
    )
    .unwrap();
    KisChartSource::new(rest)
}

fn token_ok() -> serde_json::Value {
    serde_json::json!({
        "access_token": "token-1",
        "access_token_token_expired": "2099-01-01 10:00:00",
    })
}

async fn mount_token(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/oauth2/tokenP"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_ok()))
        .mount(server)
        .await;
}

fn chart_body(dates_prices: &[(&str, &str, i64)]) -> serde_json::Value {
    let rows: Vec<serde_json::Value> = dates_prices
        .iter()
        .map(|(date, close, volume)| {
            serde_json::json!({
                "stck_bsop_date": date,
                "stck_oprc": close,
                "stck_hgpr": close,
                "stck_lwpr": close,
                "stck_clpr": close,
                "acml_vol": volume.to_string(),
            })
        })
        .collect();
    serde_json::json!({ "rt_cd": "0", "output1": {}, "output2": rows })
}

fn instant(day: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&format!("{day}T06:30:00+00:00"))
        .unwrap()
        .with_timezone(&Utc)
}

/// A 250-calendar-day request must split into 100/100/50-day windows whose
/// results merge oldest-first — the endpoint caps pages at 100 rows and has
/// no tr_cont continuation.
#[tokio::test]
async fn multi_window_backfill_merges_ascending() {
    let server = MockServer::start().await;
    mount_token(&server).await;

    let windows = [
        (
            "20250101",
            "20250410",
            vec![("20250102", "100", 1), ("20250103", "101", 2)],
        ),
        ("20250411", "20250719", vec![("20250414", "104", 3)]),
        ("20250720", "20250908", vec![("20250721", "108", 4)]),
    ];
    for (from, to, rows) in windows {
        let body = chart_body(&rows);
        Mock::given(method("GET"))
            .and(path(
                "/uapi/domestic-stock/v1/quotations/inquire-daily-itemchartprice",
            ))
            .and(query_param("FID_INPUT_DATE_1", from))
            .and(query_param("FID_INPUT_DATE_2", to))
            .and(query_param("FID_PERIOD_DIV_CODE", "D"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .expect(1)
            .mount(&server)
            .await;
    }

    let source = source(server.uri()).await;
    let bars = source
        .daily_bars(&symbol(), instant("2025-01-01"), instant("2025-09-08"))
        .await
        .unwrap();

    assert_eq!(bars.len(), 4);
    let closes: Vec<String> = bars.iter().map(|bar| bar.close.to_string()).collect();
    assert_eq!(closes, ["100", "101", "104", "108"]); // oldest first
    assert_eq!(bars[0].volume, 1);
    assert_eq!(bars[3].volume, 4);
    assert_eq!(bars[0].open.to_string(), "100"); // OHLC parsed, not just close
}

/// A window that answers with a full 100-row page must be split and re-asked
/// — dropping older bars silently is the failure this guards.
#[tokio::test]
async fn full_page_triggers_window_split() {
    let server = MockServer::start().await;
    mount_token(&server).await;

    // Build a 100-row page of consecutive dates starting 2025-01-02.
    let mut start = chrono::NaiveDate::from_ymd_opt(2025, 1, 2).unwrap();
    let mut rows = Vec::new();
    for _ in 0..100 {
        rows.push((start.format("%Y%m%d").to_string(), "100".to_string(), 1i64));
        start += chrono::Duration::days(1);
    }
    let rows_ref: Vec<(&str, &str, i64)> = rows
        .iter()
        .map(|(d, c, v)| (d.as_str(), c.as_str(), *v))
        .collect();
    let full_page = chart_body(&rows_ref);

    // Whole window [0101..0410] answers with a full 100-row page.
    Mock::given(method("GET"))
        .and(path(
            "/uapi/domestic-stock/v1/quotations/inquire-daily-itemchartprice",
        ))
        .and(query_param("FID_INPUT_DATE_1", "20250101"))
        .and(query_param("FID_INPUT_DATE_2", "20250410"))
        .respond_with(ResponseTemplate::new(200).set_body_json(full_page))
        .expect(1)
        .mount(&server)
        .await;

    // 99 days halve at +49 → [0101..0219] and [0220..0410]; each half answers
    // with one in-range row.
    let halves = [
        ("20250101", "20250219", "20250105"),
        ("20250220", "20250410", "20250301"),
    ];
    for (from, to, bar_date) in halves {
        Mock::given(method("GET"))
            .and(path(
                "/uapi/domestic-stock/v1/quotations/inquire-daily-itemchartprice",
            ))
            .and(query_param("FID_INPUT_DATE_1", from))
            .and(query_param("FID_INPUT_DATE_2", to))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(chart_body(&[(bar_date, "105", 9)])),
            )
            .expect(1)
            .mount(&server)
            .await;
    }

    let source = source(server.uri()).await;
    let bars = source
        .daily_bars(&symbol(), instant("2025-01-01"), instant("2025-04-10"))
        .await
        .unwrap();
    assert_eq!(bars.len(), 2); // halves merged, deduped
}

/// Business rejection (rt_cd != 0) surfaces as a source error with the msg.
#[tokio::test]
async fn business_rejection_fails_loud() {
    let server = MockServer::start().await;
    mount_token(&server).await;

    Mock::given(method("GET"))
        .and(path(
            "/uapi/domestic-stock/v1/quotations/inquire-daily-itemchartprice",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "rt_cd": "8", "msg_cd": "IGW00201", "msg1": "no permission (FC020)"
        })))
        .mount(&server)
        .await;

    let source = source(server.uri()).await;
    let error = source
        .daily_bars(&symbol(), instant("2025-01-01"), instant("2025-01-31"))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("IGW00201"), "got: {error}");
}

/// Window trimming: bars outside [from, to] close instants are excluded.
#[tokio::test]
async fn window_bounds_are_inclusive_and_trimmed() {
    let server = MockServer::start().await;
    mount_token(&server).await;

    Mock::given(method("GET"))
        .and(path(
            "/uapi/domestic-stock/v1/quotations/inquire-daily-itemchartprice",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(chart_body(&[
            ("20250102", "100", 1),
            ("20250103", "101", 2),
            ("20250104", "102", 3),
        ])))
        .mount(&server)
        .await;

    let source = source(server.uri()).await;
    let bars = source
        .daily_bars(
            &symbol(),
            instant("2025-01-03"),
            instant("2025-01-03") + Duration::hours(1),
        )
        .await
        .unwrap();
    assert_eq!(bars.len(), 1);
    assert_eq!(bars[0].close.to_string(), "101");
}
