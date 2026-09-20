//! 국내주식 기간별시세(일/주/월/년) — `MarketDataSource` implementation.
//!
//! Verified against the official sample repository (examples_llm/
//! domestic_stock/inquire_daily_itemchartprice):
//! - GET `/uapi/domestic-stock/v1/quotations/inquire-daily-itemchartprice`
//! - tr_id `FHKST03010100` (실전 = 모의 — no V prefix)
//! - query: FID_COND_MRKT_DIV_CODE=J, FID_INPUT_ISCD, FID_INPUT_DATE_1/2
//!   (YYYYMMDD), FID_PERIOD_DIV_CODE=D, FID_ORG_ADJ_PRC
//! - **max 100 rows per call** — this endpoint has no tr_cont pagination;
//!   long ranges are chunked into non-overlapping 100-calendar-day windows
//!   (~68 trading bars each). A window that still returns a full 100 rows
//!   is split recursively so no bar can be silently dropped.
//! - output2 rows: stck_bsop_date (YYYYMMDD), stck_oprc/stck_hgpr/
//!   stck_lwpr/stck_clpr, acml_vol — string fields; missing values fail
//!   loudly (wiki: kis-api, fail-loud).

use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use rust_decimal::Decimal;
use serde_json::Value;
use std::str::FromStr;

use crate::kis::rest::KisRestClient;
use crate::marketdata::{Bar, MarketDataError, MarketDataSource, Timeframe};
use crate::types::Symbol;

const PATH: &str = "/uapi/domestic-stock/v1/quotations/inquire-daily-itemchartprice";
const TR_ID: &str = "FHKST03010100";
/// ~68 trading bars per window — comfortably under the 100-row cap.
const WINDOW_DAYS: i64 = 100;
const MAX_SPLIT_DEPTH: u8 = 6;
/// A full page means the window may be hiding older bars.
const PAGE_LIMIT: usize = 100;

/// KIS daily-bar source over an existing [`KisRestClient`]. Prices default to
/// 수정주가 (adjusted) so indicator series stay continuous across 권리락.
pub struct KisChartSource {
    rest: KisRestClient,
    /// "0" = 수정주가 (default), "1" = 원주가.
    org_adj_prc: &'static str,
}

impl KisChartSource {
    pub fn new(rest: KisRestClient) -> Self {
        Self {
            rest,
            org_adj_prc: "0",
        }
    }

    /// One HTTP page fetch: request a single window, parse, sort, dedup.
    async fn fetch_page(
        &self,
        symbol: &Symbol,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<Vec<Bar>, MarketDataError> {
        let query = [
            ("FID_COND_MRKT_DIV_CODE", "J".to_string()),
            ("FID_INPUT_ISCD", symbol.as_str().to_string()),
            ("FID_INPUT_DATE_1", start.format("%Y%m%d").to_string()),
            ("FID_INPUT_DATE_2", end.format("%Y%m%d").to_string()),
            ("FID_PERIOD_DIV_CODE", "D".to_string()),
            ("FID_ORG_ADJ_PRC", self.org_adj_prc.to_string()),
        ];
        let (body, _) = self
            .rest
            .request(reqwest::Method::GET, PATH, TR_ID, None, Some(&query), None)
            .await
            .map_err(|error| MarketDataError::Source(error.to_string()))?;

        let rows = body
            .get("output2")
            .and_then(Value::as_array)
            .ok_or_else(|| MarketDataError::Source("response missing output2 array".into()))?;

        let mut bars: Vec<Bar> = rows
            .iter()
            .map(|row| parse_row(symbol, row))
            .collect::<Result<Vec<_>, _>>()?;
        bars.sort_by_key(|bar| bar.timestamp);
        bars.dedup_by(|a, b| a.timestamp == b.timestamp);
        Ok(bars)
    }
}

fn parse_row(symbol: &Symbol, row: &Value) -> Result<Bar, MarketDataError> {
    let field = |name: &str| -> Result<&str, MarketDataError> {
        row.get(name)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                MarketDataError::Source(format!("bar row missing field {name:?}: {row}"))
            })
    };
    let price = |name: &str| -> Result<Decimal, MarketDataError> {
        Decimal::from_str(field(name)?)
            .map_err(|error| MarketDataError::Source(format!("bad price in {name}: {error}")))
    };

    let date = NaiveDate::parse_from_str(field("stck_bsop_date")?, "%Y%m%d")
        .map_err(|error| MarketDataError::Source(format!("bad stck_bsop_date: {error}")))?;
    // The bar's completion instant: KST 15:30 close == 06:30 UTC same date.
    let timestamp = Utc.from_utc_datetime(&date.and_hms_opt(6, 30, 0).expect("valid hm"));

    Ok(Bar {
        symbol: symbol.clone(),
        timeframe: Timeframe::Day,
        timestamp,
        open: price("stck_oprc")?,
        high: price("stck_hgpr")?,
        low: price("stck_lwpr")?,
        close: price("stck_clpr")?,
        volume: field("acml_vol")?
            .parse::<i64>()
            .map_err(|error| MarketDataError::Source(format!("bad acml_vol: {error}")))?,
    })
}

#[async_trait::async_trait]
impl MarketDataSource for KisChartSource {
    async fn daily_bars(
        &self,
        symbol: &Symbol,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<Bar>, MarketDataError> {
        let (from_date, end_date) = (from.date_naive(), to.date_naive());
        if from_date > end_date {
            return Ok(Vec::new());
        }

        // Walk non-overlapping 100-day windows; a page that comes back full
        // (100 rows) may hide older bars, so subdivide it — an explicit work
        // stack instead of recursion (async fns cannot recurse unboxed).
        let mut pending: Vec<(NaiveDate, NaiveDate, u8)> = Vec::new();
        let mut start = from_date;
        while start <= end_date {
            let window_end = (start + Duration::days(WINDOW_DAYS - 1)).min(end_date);
            pending.push((start, window_end, 0));
            start = window_end + Duration::days(1);
        }

        let mut all = Vec::new();
        while let Some((window_start, window_end, depth)) = pending.pop() {
            let bars = self.fetch_page(symbol, window_start, window_end).await?;
            if bars.len() >= PAGE_LIMIT && depth < MAX_SPLIT_DEPTH && window_start < window_end {
                let mid = window_start + Duration::days((window_end - window_start).num_days() / 2);
                pending.push((window_start, mid, depth + 1));
                pending.push((mid + Duration::days(1), window_end, depth + 1));
                continue; // page discarded; halves re-fetch it entirely
            }
            all.extend(bars);
        }
        all.sort_by_key(|bar| bar.timestamp);
        all.dedup_by(|a, b| a.timestamp == b.timestamp);
        all.retain(|bar| bar.timestamp >= from && bar.timestamp <= to);
        Ok(all)
    }
}
