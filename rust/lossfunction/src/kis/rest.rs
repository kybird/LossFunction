//! KIS REST client for the domestic-stock endpoints (wiki: kis-api).
//!
//! Verified shapes: tr_id per endpoint/environment, UPPERCASE string bodies,
//! inquire-balance pagination via the tr_cont response header, rt_cd != 0 =
//! business rejection (never retried), 401 → one token re-issue then retry.

use chrono::Utc;
use rust_decimal::Decimal;
use serde_json::Value;

use crate::broker::{BrokerError, ExecutionReport, OrderAck, OrderRequest, Position};
use crate::kis::auth::KisAuth;
use crate::types::{OrderSide, OrderType, Price, Symbol};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiErrorKind {
    ApiReject,   // HTTP 200 but rt_cd != 0 — retrying cannot help
    Auth,        // 401/403 — token re-issue, then give up
    RateLimited, // 429
    Server,      // 5xx
    Network,     // transport
    Malformed,   // unusable body
}

#[derive(Debug, thiserror::Error)]
#[error("{kind:?}: {message}")]
pub struct ApiError {
    pub kind: ApiErrorKind,
    pub message: String,
    pub msg_cd: Option<String>,
}

impl From<ApiError> for BrokerError {
    fn from(error: ApiError) -> Self {
        match error.kind {
            ApiErrorKind::ApiReject => BrokerError::Rejected(error.to_string()),
            _ => BrokerError::Internal(error.to_string()),
        }
    }
}

pub struct KisRestClient {
    http: reqwest::Client,
    auth: KisAuth,
    environment: String,
    base_url: String,
    cano: String,
    prdt: String,
    /// Recorded into order traces (audit mode tagging).
    trading_mode: String,
}

fn parse_account_number(account: &str) -> Result<(String, String), BrokerError> {
    let digits: String = account.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() != 10 {
        return Err(BrokerError::Internal(format!(
            "KIS account number must be 10 digits (8-2), got {account:?}"
        )));
    }
    Ok((digits[..8].to_string(), digits[8..].to_string()))
}

fn ord_dvsn(order_type: OrderType) -> &'static str {
    match order_type {
        OrderType::Limit => "00",
        OrderType::Market => "01",
    }
}

impl KisRestClient {
    pub fn new(
        auth: KisAuth,
        account_number: &str,
        environment: &str,
        base_url: String,
        http: reqwest::Client,
        trading_mode: &str,
    ) -> Result<Self, BrokerError> {
        let (cano, prdt) = parse_account_number(account_number)?;
        Ok(Self {
            http,
            auth,
            environment: environment.to_string(),
            base_url,
            cano,
            prdt,
            trading_mode: trading_mode.to_string(),
        })
    }

    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        tr_id: &str,
        body: Option<Value>,
        query: Option<&[(&str, String)]>,
        tr_cont: Option<&str>,
    ) -> Result<(Value, Option<String>), ApiError> {
        let mut reissued = false;
        loop {
            let token = self.auth.access_token().await.map_err(|error| ApiError {
                kind: ApiErrorKind::Auth,
                message: error.to_string(),
                msg_cd: None,
            })?;
            let url = format!("{}{path}", self.base_url);
            let mut request = self
                .http
                .request(method.clone(), &url)
                .header("authorization", format!("Bearer {token}"))
                .header("appkey", "")
                .header("tr_id", tr_id)
                .header("custtype", "P")
                .header("tr_cont", tr_cont.unwrap_or(""));
            if let Some(query) = query {
                request = request.query(query);
            }
            if let Some(body) = &body {
                request = request.json(body);
            }
            let response = request.send().await.map_err(|error| ApiError {
                kind: ApiErrorKind::Network,
                message: error.to_string(),
                msg_cd: None,
            })?;

            match response.status().as_u16() {
                200 => {}
                401 | 403 if !reissued => {
                    reissued = true;
                    self.auth.invalidate().await;
                    continue;
                }
                401 | 403 => {
                    return Err(ApiError {
                        kind: ApiErrorKind::Auth,
                        message: "token rejected after re-issue".to_string(),
                        msg_cd: None,
                    })
                }
                429 => {
                    return Err(ApiError {
                        kind: ApiErrorKind::RateLimited,
                        message: "rate limited".to_string(),
                        msg_cd: None,
                    })
                }
                status if status >= 500 => {
                    return Err(ApiError {
                        kind: ApiErrorKind::Server,
                        message: format!("server error {status}"),
                        msg_cd: None,
                    })
                }
                status => {
                    return Err(ApiError {
                        kind: ApiErrorKind::Malformed,
                        message: format!("unexpected status {status}"),
                        msg_cd: None,
                    })
                }
            }

            let tr_cont_header = response
                .headers()
                .get("tr_cont")
                .and_then(|value| value.to_str().ok())
                .map(str::to_string);
            let payload: Value = response.json().await.map_err(|error| ApiError {
                kind: ApiErrorKind::Malformed,
                message: format!("non-JSON response: {error}"),
                msg_cd: None,
            })?;
            if payload["rt_cd"] == "0" || payload.get("rt_cd").is_none() {
                return Ok((payload, tr_cont_header));
            }
            return Err(ApiError {
                kind: ApiErrorKind::ApiReject,
                message: format!(
                    "KIS rejected: rt_cd={} msg_cd={} msg1={}",
                    payload["rt_cd"], payload["msg_cd"], payload["msg1"]
                ),
                msg_cd: payload["msg_cd"].as_str().map(str::to_string),
            });
        }
    }

    /// Submit a domestic-stock cash order; returns the ack plus the raw
    /// output (carries KRX_FWDG_ORD_ORGNO for later cancellation).
    pub async fn submit_cash_order(
        &self,
        order: &OrderRequest,
    ) -> Result<(OrderAck, Value), ApiError> {
        let tr_id = match (self.environment.as_str(), order.side) {
            ("real", OrderSide::Buy) => "TTTC0012U",
            ("real", OrderSide::Sell) => "TTTC0011U",
            (_, OrderSide::Buy) => "VTTC0012U",
            (_, OrderSide::Sell) => "VTTC0011U",
        };
        let body = serde_json::json!({
            "CANO": self.cano,
            "ACNT_PRDT_CD": self.prdt,
            "PDNO": order.symbol.as_str(),
            "ORD_DVSN": ord_dvsn(order.order_type),
            "ORD_QTY": order.quantity.to_string(),
            "ORD_UNPR": match order.limit_price {
                Some(price) => price.to_string(),
                None => "0".to_string(),
            },
            "EXCG_ID_DVSN_CD": "KRX",
            "SLL_TYPE": "",
            "CNDT_PRIC": "",
        });
        let (payload, _) = self
            .request(
                reqwest::Method::POST,
                "/uapi/domestic-stock/v1/trading/order-cash",
                tr_id,
                Some(body),
                None,
                None,
            )
            .await?;
        let odno = payload["output"]["ODNO"]
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or(ApiError {
                kind: ApiErrorKind::Malformed,
                message: "order-cash response missing output.ODNO".to_string(),
                msg_cd: None,
            })?
            .to_string();
        Ok((
            OrderAck {
                client_order_id: order.client_order_id.clone(),
                broker_order_id: odno,
            },
            payload["output"].clone(),
        ))
    }

    /// Cancel the full remainder of an order (order-rvsecncl).
    pub async fn cancel_cash_order(
        &self,
        broker_order_id: &str,
        orgno: &str,
        ord_dvsn: &str,
    ) -> Result<(), ApiError> {
        let tr_id = if self.environment == "real" {
            "TTTC0013U"
        } else {
            "VTTC0013U"
        };
        let body = serde_json::json!({
            "CANO": self.cano,
            "ACNT_PRDT_CD": self.prdt,
            "KRX_FWDG_ORD_ORGNO": orgno,
            "ORGN_ODNO": broker_order_id,
            "ORD_DVSN": ord_dvsn,
            "RVSE_CNCL_DVSN_CD": "02",
            "ORD_QTY": "0",
            "ORD_UNPR": "0",
            "QTY_ALL_ORD_YN": "Y",
            "EXCG_ID_DVSN_CD": "KRX",
        });
        self.request(
            reqwest::Method::POST,
            "/uapi/domestic-stock/v1/trading/order-rvsecncl",
            tr_id,
            Some(body),
            None,
            None,
        )
        .await
        .map(|_: (Value, Option<String>)| ())
    }

    /// Fetch held positions, following tr_cont M/F pagination.
    pub async fn fetch_positions(&self) -> Result<Vec<Position>, ApiError> {
        let tr_id = if self.environment == "real" {
            "TTTC8434R"
        } else {
            "VTTC8434R"
        };
        let mut rows: Vec<Value> = Vec::new();
        let mut fk = String::new();
        let mut nk = String::new();
        let mut tr_cont: Option<String> = None;
        for _ in 0..20 {
            let query = [
                ("CANO", self.cano.clone()),
                ("ACNT_PRDT_CD", self.prdt.clone()),
                ("AFHR_FLPR_YN", "N".into()),
                ("OFL_YN", String::new()),
                ("INQR_DVSN", "02".into()),
                ("UNPR_DVSN", "01".into()),
                ("FUND_STTL_ICLD_YN", "N".into()),
                ("FNCG_AMT_AUTO_RDPT_YN", "N".into()),
                ("PRCS_DVSN", "00".into()),
                ("CTX_AREA_FK100", fk.clone()),
                ("CTX_AREA_NK100", nk.clone()),
            ];
            let (body, header_cont) = self
                .request(
                    reqwest::Method::GET,
                    "/uapi/domestic-stock/v1/trading/inquire-balance",
                    tr_id,
                    None,
                    Some(&query),
                    tr_cont.as_deref(),
                )
                .await?;
            if let Some(page) = body["output1"].as_array() {
                rows.extend(page.iter().cloned());
            }
            match header_cont.as_deref() {
                Some("M") | Some("F") => {
                    fk = body["ctx_area_fk100"].as_str().unwrap_or("").to_string();
                    nk = body["ctx_area_nk100"].as_str().unwrap_or("").to_string();
                    tr_cont = Some("N".to_string());
                }
                _ => break,
            }
        }
        let mut positions = Vec::new();
        for row in rows {
            let quantity: i64 = row["hldg_qty"]
                .as_str()
                .and_then(|value| value.parse().ok())
                .unwrap_or(0);
            if quantity <= 0 {
                continue;
            }
            let symbol = row["pdno"].as_str().unwrap_or_default().to_string();
            let average = row["pchs_avg_pric"]
                .as_str()
                .and_then(|value| Decimal::from_str(value).ok())
                .unwrap_or_default();
            positions.push(Position {
                symbol: Symbol::parse(symbol.clone()).map_err(|_| ApiError {
                    kind: ApiErrorKind::Malformed,
                    message: format!("balance row bad symbol {symbol}"),
                    msg_cd: None,
                })?,
                quantity,
                average_price: average,
            });
        }
        Ok(positions)
    }

    /// Latest price snapshot (inquire-price).
    pub async fn fetch_quote(&self, symbol: &Symbol) -> Result<crate::types::Quote, ApiError> {
        let query = [
            ("FID_COND_MRKT_DIV_CODE", "J".to_string()),
            ("FID_INPUT_ISCD", symbol.as_str().to_string()),
        ];
        let (payload, _) = self
            .request(
                reqwest::Method::GET,
                "/uapi/domestic-stock/v1/quotations/inquire-price",
                "FHKST01010100",
                None,
                Some(&query),
                None,
            )
            .await?;
        let raw = payload["output"]["stck_prpr"]
            .as_str()
            .ok_or(ApiError {
                kind: ApiErrorKind::Malformed,
                message: "inquire-price missing stck_prpr".to_string(),
                msg_cd: None,
            })?
            .to_string();
        let price = Decimal::from_str(&raw).map_err(|_| ApiError {
            kind: ApiErrorKind::Malformed,
            message: format!("stck_prpr not a number: {raw}"),
            msg_cd: None,
        })?;
        Ok(crate::types::Quote {
            symbol: symbol.clone(),
            last_price: price,
            timestamp: Utc::now(),
        })
    }

    /// Today's order/execution row for one order (inquire-daily-ccld).
    pub async fn fetch_order_row(
        &self,
        broker_order_id: &str,
    ) -> Result<serde_json::Value, ApiError> {
        let today = {
            let kst = Utc::now() + chrono::TimeDelta::hours(9);
            kst.format("%Y%m%d").to_string()
        };
        let tr_id = if self.environment == "real" {
            "TTTC0081R"
        } else {
            "VTTC0081R"
        };
        let query = [
            ("CANO", self.cano.clone()),
            ("ACNT_PRDT_CD", self.prdt.clone()),
            ("INQR_STRT_DT", today.clone()),
            ("INQR_END_DT", today),
            ("SLL_BUY_DVSN_CD", "00".into()),
            ("PDNO", String::new()),
            ("CCLD_DVSN", "00".into()),
            ("INQR_DVSN", "01".into()),
            ("INQR_DVSN_3", "00".into()),
            ("ORD_GNO_BRNO", String::new()),
            ("ODNO", broker_order_id.to_string()),
            ("INQR_DVSN_1", String::new()),
            ("CTX_AREA_FK100", String::new()),
            ("CTX_AREA_NK100", String::new()),
            ("EXCG_ID_DVSN_CD", "KRX".into()),
        ];
        let (payload, _) = self
            .request(
                reqwest::Method::GET,
                "/uapi/domestic-stock/v1/trading/inquire-daily-ccld",
                tr_id,
                None,
                Some(&query),
                None,
            )
            .await?;
        if let Some(rows) = payload["output1"].as_array() {
            for row in rows {
                if row["odno"].as_str() == Some(broker_order_id) {
                    return Ok(row.clone());
                }
            }
        }
        Err(ApiError {
            kind: ApiErrorKind::Malformed,
            message: format!("order {broker_order_id} not found in ccld response"),
            msg_cd: None,
        })
    }

    /// Map a ccld row into an execution report; fails loud on missing fields.
    pub fn execution_report_from_row(
        &self,
        broker_order_id: &str,
        client_order_id: &str,
        row: &Value,
    ) -> Result<ExecutionReport, ApiError> {
        let field = |name: &str| -> Result<String, ApiError> {
            row[name].as_str().map(str::to_string).ok_or(ApiError {
                kind: ApiErrorKind::Malformed,
                message: format!("ccld row missing {name}"),
                msg_cd: None,
            })
        };
        let symbol = Symbol::parse(field("pdno")?).map_err(|_| ApiError {
            kind: ApiErrorKind::Malformed,
            message: "ccld row bad pdno".to_string(),
            msg_cd: None,
        })?;
        let order_quantity: i64 = field("ord_qty")?.parse().map_err(|_| ApiError {
            kind: ApiErrorKind::Malformed,
            message: "ccld row bad ord_qty".to_string(),
            msg_cd: None,
        })?;
        let filled: i64 = row["tot_ccld_qty"]
            .as_str()
            .unwrap_or("0")
            .parse()
            .unwrap_or(0);
        let cancelled = row["cncl_yn"].as_str() == Some("Y");
        let average_fill_price: Option<Price> = if filled > 0 {
            let amount = field("tot_ccld_amt")?;
            Some(
                Decimal::from_str(&amount).map_err(|_| ApiError {
                    kind: ApiErrorKind::Malformed,
                    message: format!("ccld row bad tot_ccld_amt: {amount}"),
                    msg_cd: None,
                })? / Decimal::from(filled),
            )
        } else {
            None
        };
        Ok(ExecutionReport {
            broker_order_id: broker_order_id.to_string(),
            client_order_id: client_order_id.to_string(),
            symbol,
            side: if field("sll_buy_dvsn_cd")? == "02" {
                OrderSide::Buy
            } else {
                OrderSide::Sell
            },
            order_type: if row["ord_dvsn_cd"].as_str() == Some("00") {
                OrderType::Limit
            } else {
                OrderType::Market
            },
            order_quantity,
            filled_quantity: filled,
            average_fill_price,
            open: !cancelled && filled < order_quantity,
            timestamp: Utc::now(),
        })
    }
}

use std::str::FromStr;
