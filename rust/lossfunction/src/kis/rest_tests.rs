//! wiremock contract tests for the KIS REST client (wiki: kis-api shapes).

use crate::broker::{OrderAck, OrderRequest};
use crate::kis::auth::KisAuth;
use crate::kis::rest::KisRestClient;
use crate::types::{OrderSide, OrderType, Symbol};
use rust_decimal::Decimal;
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn symbol() -> Symbol {
    Symbol::parse("005930").unwrap()
}

fn order() -> OrderRequest {
    OrderRequest {
        client_order_id: "c-1".into(),
        symbol: symbol(),
        side: OrderSide::Buy,
        order_type: OrderType::Market,
        quantity: 10,
        limit_price: None,
    }
}

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
        "mock",
        base_url,
        reqwest::Client::new(),
        "paper",
    )
    .unwrap()
}

fn token_ok() -> serde_json::Value {
    serde_json::json!({
        "access_token": "token-1",
        "access_token_token_expired": "2099-01-01 10:00:00",
    })
}

fn order_ok() -> serde_json::Value {
    serde_json::json!({
        "rt_cd": "0", "msg_cd": "00220000", "msg1": "주문 전송 완료",
        "output": {"ODNO": "00001234", "ORD_TMD": "093012", "KRX_FWDG_ORD_ORGNO": "01290"},
    })
}

#[tokio::test]
async fn submit_order_builds_official_request_shape() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth2/tokenP"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_ok()))
        .mount(&server)
        .await;
    let expected_body = serde_json::json!({
        "CANO": "12345678",
        "ACNT_PRDT_CD": "01",
        "PDNO": "005930",
        "ORD_DVSN": "01",
        "ORD_QTY": "10",
        "ORD_UNPR": "0",
        "EXCG_ID_DVSN_CD": "KRX",
        "SLL_TYPE": "",
        "CNDT_PRIC": "",
    });
    Mock::given(method("POST"))
        .and(path("/uapi/domestic-stock/v1/trading/order-cash"))
        .and(body_json(&expected_body))
        .and(header("tr_id", "VTTC0012U"))
        .and(header("custtype", "P"))
        .and(header("authorization", "Bearer token-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(order_ok()))
        .expect(1)
        .mount(&server)
        .await;

    let kis = client(server.uri()).await;
    let (ack, output) = kis.submit_cash_order(&order()).await.unwrap();
    assert_eq!(ack.client_order_id, "c-1");
    assert_eq!(ack.broker_order_id, "00001234");
    assert_eq!(output["KRX_FWDG_ORD_ORGNO"], "01290");
}

#[tokio::test]
async fn business_rejection_is_classified_and_never_retried() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth2/tokenP"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_ok()))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/uapi/domestic-stock/v1/trading/order-cash"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "rt_cd": "1", "msg_cd": "40150", "msg1": "주문수량 오류",
        })))
        .expect(1)
        .mount(&server)
        .await;

    let kis = client(server.uri()).await;
    let error = kis.submit_cash_order(&order()).await.unwrap_err();
    assert_eq!(error.kind, crate::kis::rest::ApiErrorKind::ApiReject);
    assert_eq!(error.msg_cd.as_deref(), Some("40150"));
}

#[tokio::test]
async fn balance_pagination_follows_tr_cont() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth2/tokenP"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_ok()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/uapi/domestic-stock/v1/trading/inquire-balance"))
        .and(query_param("CTX_AREA_NK100", ""))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("tr_cont", "M")
                .set_body_json(serde_json::json!({
                    "rt_cd": "0",
                    "ctx_area_fk100": "FK",
                    "ctx_area_nk100": "NK1",
                    "output1": [
                        {"pdno": "005930", "hldg_qty": "10", "pchs_avg_pric": "80000.0000"},
                    ],
                })),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/uapi/domestic-stock/v1/trading/inquire-balance"))
        .and(query_param("CTX_AREA_NK100", "NK1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "rt_cd": "0",
            "ctx_area_fk100": "",
            "ctx_area_nk100": "",
            "output1": [
                {"pdno": "035420", "hldg_qty": "5", "pchs_avg_pric": "41000.0000"},
                {"pdno": "069500", "hldg_qty": "0", "pchs_avg_pric": "0"},
            ],
        })))
        .expect(1)
        .mount(&server)
        .await;

    let kis = client(server.uri()).await;
    let positions = kis.fetch_positions().await.unwrap();
    assert_eq!(positions.len(), 2); // zero-quantity row filtered
    assert_eq!(positions[0].symbol.as_str(), "005930");
    assert_eq!(positions[0].quantity, 10);
    assert_eq!(positions[1].symbol.as_str(), "035420");
}

#[tokio::test]
async fn quote_parses_stck_prpr() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth2/tokenP"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_ok()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/uapi/domestic-stock/v1/quotations/inquire-price"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "rt_cd": "0",
            "output": {"stck_shrn_iscd": "005930", "stck_prpr": "80500"},
        })))
        .expect(1)
        .mount(&server)
        .await;

    let kis = client(server.uri()).await;
    let quote = kis.fetch_quote(&symbol()).await.unwrap();
    assert_eq!(quote.last_price, Decimal::from(80_500));
}

#[tokio::test]
async fn ccld_row_maps_to_execution_report() {
    let kis = client("http://127.0.0.1:9".into()).await;
    let row = serde_json::json!({
        "odno": "00001234", "pdno": "005930", "ord_qty": "10",
        "tot_ccld_qty": "10", "tot_ccld_amt": "790000",
        "cncl_yn": "N", "sll_buy_dvsn_cd": "02", "ord_dvsn_cd": "00",
    });
    let report = kis
        .execution_report_from_row("00001234", "c-1", &row)
        .unwrap();
    assert_eq!(report.order_quantity, 10);
    assert_eq!(report.filled_quantity, 10);
    assert_eq!(report.average_fill_price, Some(Decimal::from(79_000)));
    assert!(!report.open);
    assert_eq!(report.side, OrderSide::Buy);
    assert_eq!(report.order_type, OrderType::Limit);

    // Missing field fails loud with the field name.
    let broken = kis
        .execution_report_from_row("x", "c", &serde_json::json!({"pdno": "005930"}))
        .unwrap_err();
    assert!(broken.message.contains("ord_qty"));
}
