//! KIS WebSocket market-data client (wiki: kis-api).
//!
//! Verified shapes: subscribe message
//! `{"header": {"approval_key", "tr_type": "1"|"0", "custtype": "P"},
//!   "body": {"input": {"tr_id", "tr_key"}}}`;
//! data frames are pipe-delimited `0|TR_ID|TR_KEY|v1^v2^...` where the 4th
//! field repeats ^-separated records of the H0STCNT0 column list (46 cols);
//! keepalive arrives as a JSON message with header.tr_id == "PINGPONG".
//! Desired subscriptions live in this client (domain state), NOT in the
//! connection — a reconnect replays them, so losing a connection can never
//! silently stop market data.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::{FixedOffset, NaiveDateTime, Utc};
use futures_util::SinkExt;
use rust_decimal::Decimal;
use serde_json::Value;
use tokio::sync::Mutex;

use crate::types::{Quote, Symbol};

/// 국내주식 실시간체결가(KRX) H0STCNT0 — measured column order (47 columns).
/// The official guide documents 46, but the live stream carries one more
/// trailing column (measured 2026-10-02, value "2", semantics unverified) —
/// the third pipe field is the record COUNT ("001"|"003"), not the tr_key.
pub const H0STCNT0_COLUMNS: [&str; 47] = [
    "MKSC_SHRN_ISCD",
    "STCK_CNTG_HOUR",
    "STCK_PRPR",
    "PRDY_VRSS_SIGN",
    "PRDY_VRSS",
    "PRDY_CTRT",
    "WGHN_AVRG_STCK_PRC",
    "STCK_OPRC",
    "STCK_HGPR",
    "STCK_LWPR",
    "ASKP1",
    "BIDP1",
    "CNTG_VOL",
    "ACML_VOL",
    "ACML_TR_PBMN",
    "SELN_CNTG_CSNU",
    "SHNU_CNTG_CSNU",
    "NTBY_CNTG_CSNU",
    "CTTR",
    "SELN_CNTG_SMTN",
    "SHNU_CNTG_SMTN",
    "CCLD_DVSN",
    "SHNU_RATE",
    "PRDY_VOL_VRSS_ACML_VOL_RATE",
    "OPRC_HOUR",
    "OPRC_VRSS_PRPR_SIGN",
    "OPRC_VRSS_PRPR",
    "HGPR_HOUR",
    "HGPR_VRSS_PRPR_SIGN",
    "HGPR_VRSS_PRPR",
    "LWPR_HOUR",
    "LWPR_VRSS_PRPR_SIGN",
    "LWPR_VRSS_PRPR",
    "BSOP_DATE",
    "NEW_MKOP_CLS_CODE",
    "TRHT_YN",
    "ASKP_RSQN1",
    "BIDP_RSQN1",
    "TOTAL_ASKP_RSQN",
    "TOTAL_BIDP_RSQN",
    "VOL_TNRT",
    "PRDY_SMNS_HOUR_ACML_VOL",
    "PRDY_SMNS_HOUR_ACML_VOL_RATE",
    "HOUR_CLS_CODE",
    "MRKT_TRTM_CLS_CODE",
    "VI_STND_PRC",
    // Undocumented trailing column observed in the live stream (2026-10-02):
    // every H0STCNT0 record carries a 47th value ("2" during the session).
    // Kept explicit so the width check stays honest; semantics unknown.
    "UNDOCUMENTED_FIELD_47",
];

const COLUMN_INDEX: fn(&str) -> Option<usize> =
    |name| H0STCNT0_COLUMNS.iter().position(|column| *column == name);

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("unexpected market-data frame: {0:?}")]
    UnexpectedFrame(String),
    #[error("frame field count {count} not a multiple of {width}")]
    NotMultiple { count: usize, width: usize },
    #[error("bad field {field}: {message}")]
    BadField {
        field: &'static str,
        message: String,
    },
}

fn kst() -> FixedOffset {
    FixedOffset::east_opt(9 * 3600).unwrap()
}

/// KRX regular session in KST (09:00–15:30 weekdays) — the same rule the
/// realtime loop uses to gate fills, shared here for the tick-stall watchdog.
pub fn is_regular_session_utc(timestamp: chrono::DateTime<chrono::Utc>) -> bool {
    use chrono::{Datelike, Timelike};
    let kst = timestamp + chrono::Duration::hours(9);
    let minutes = kst.hour() * 60 + kst.minute();
    let weekday = kst.weekday().num_days_from_monday();
    weekday < 5 && (9 * 60..=15 * 60 + 30).contains(&minutes)
}

/// Parse one H0STCNT0 data frame into `Quote` domain events.
pub fn parse_market_data(raw: &str) -> Result<Vec<Quote>, FrameError> {
    let fields: Vec<&str> = raw.split('|').collect();
    if fields.len() < 4 || fields[1] != "H0STCNT0" {
        return Err(FrameError::UnexpectedFrame(raw.chars().take(80).collect()));
    }
    let values: Vec<&str> = fields[3].split('^').collect();
    let width = H0STCNT0_COLUMNS.len();
    if !values.len().is_multiple_of(width) {
        return Err(FrameError::NotMultiple {
            count: values.len(),
            width,
        });
    }

    let symbol_index = COLUMN_INDEX("MKSC_SHRN_ISCD").unwrap();
    let price_index = COLUMN_INDEX("STCK_PRPR").unwrap();
    let time_index = COLUMN_INDEX("STCK_CNTG_HOUR").unwrap();
    let date_index = COLUMN_INDEX("BSOP_DATE").unwrap();

    let mut quotes = Vec::new();
    for record in values.chunks(width) {
        let symbol = Symbol::parse(record[symbol_index]).map_err(|_| FrameError::BadField {
            field: "MKSC_SHRN_ISCD",
            message: record[symbol_index].to_string(),
        })?;
        let price: Decimal = record[price_index]
            .parse()
            .map_err(|_| FrameError::BadField {
                field: "STCK_PRPR",
                message: record[price_index].to_string(),
            })?;
        let timestamp = NaiveDateTime::parse_from_str(
            &format!("{}{}", record[date_index], record[time_index]),
            "%Y%m%d%H%M%S",
        )
        .map(|naive| naive.and_local_timezone(kst()).unwrap().with_timezone(&Utc))
        .map_err(|_| FrameError::BadField {
            field: "BSOP_DATE+STCK_CNTG_HOUR",
            message: format!("{} {}", record[date_index], record[time_index]),
        })?;
        quotes.push(Quote {
            symbol,
            last_price: price,
            timestamp,
        });
    }
    Ok(quotes)
}

/// Build the KIS subscribe/unsubscribe message (official shape).
pub fn build_subscribe_message(
    approval_key: &str,
    tr_id: &str,
    tr_key: &str,
    subscribe: bool,
) -> Value {
    serde_json::json!({
        "header": {
            "approval_key": approval_key,
            "tr_type": if subscribe { "1" } else { "0" },
            "custtype": "P",
        },
        "body": {"input": {"tr_id": tr_id, "tr_key": tr_key}},
    })
}

/// Detect the server keepalive (JSON header tr_id PINGPONG).
pub fn is_pingpong(raw: &str) -> bool {
    serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(|payload| payload["header"]["tr_id"].as_str().map(str::to_string))
        .is_some_and(|tr_id| tr_id == "PINGPONG")
        || raw.contains("PINGPONG")
}

/// Approval key issuance (POST /oauth2/Approval — note the `secretkey`
/// field name, unlike the REST token's `appsecret`).
pub async fn fetch_approval_key(
    http: &reqwest::Client,
    base_url: &str,
    app_key: &str,
    app_secret: &str,
) -> Result<String, String> {
    let response = http
        .post(format!("{base_url}/oauth2/Approval"))
        .json(&serde_json::json!({
            "grant_type": "client_credentials",
            "appkey": app_key,
            "secretkey": app_secret,
        }))
        .send()
        .await
        .map_err(|error| format!("approval key request failed: {error}"))?;
    let payload: Value = response
        .json()
        .await
        .map_err(|error| format!("approval key response unreadable: {error}"))?;
    payload["approval_key"]
        .as_str()
        .filter(|key| !key.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "approval key missing from response".to_string())
}

/// Minimal async text stream over one WebSocket connection.
#[async_trait::async_trait]
pub trait WsConnection: Send + Sync {
    async fn send(&self, message: &str) -> Result<(), String>;
    async fn recv(&self) -> Result<String, String>;
    fn pong(&self, data: &str);
}

/// Creates connections; real implementation wraps tokio-tungstenite, tests
/// provide scripted fakes.
#[async_trait::async_trait]
pub trait ConnectionFactory: Send + Sync {
    async fn connect(&self, url: &str) -> Result<Arc<dyn WsConnection>, String>;
}

type TungsteniteStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Real transport over tokio-tungstenite. The KIS realtime endpoint is plain
/// `ws://`, so the TLS-free feature set is sufficient. A disconnect surfaces
/// as a `recv` error; the client's reconnect loop then replays subscriptions.
pub struct TungsteniteConnection {
    sink: Arc<Mutex<futures_util::stream::SplitSink<TungsteniteStream, WsMessage>>>,
    stream: Arc<Mutex<futures_util::stream::SplitStream<TungsteniteStream>>>,
}

type WsMessage = tokio_tungstenite::tungstenite::Message;

#[async_trait::async_trait]
impl WsConnection for TungsteniteConnection {
    async fn send(&self, message: &str) -> Result<(), String> {
        let mut sink = self.sink.lock().await;
        sink.send(WsMessage::Text(message.to_string().into()))
            .await
            .map_err(|error| format!("ws send failed: {error}"))
    }

    async fn recv(&self) -> Result<String, String> {
        let mut stream = self.stream.lock().await;
        use futures_util::StreamExt;
        loop {
            match stream.next().await {
                Some(Ok(WsMessage::Text(text))) => return Ok(text.to_string()),
                Some(Ok(WsMessage::Ping(payload))) => {
                    // Protocol-level ping: answer inline so the peer keeps us.
                    let mut sink = self.sink.lock().await;
                    let _ = sink.send(WsMessage::Pong(payload)).await;
                }
                // Binary/pong frames — KIS market data speaks text only.
                Some(Ok(_)) => continue,
                Some(Err(error)) => return Err(format!("ws recv failed: {error}")),
                None => return Err("ws stream closed".to_string()),
            }
        }
    }

    fn pong(&self, data: &str) {
        // KIS application-level keepalive (JSON header.tr_id == PINGPONG):
        // echo the message back. Fire-and-forget — the trait is sync and the
        // reply is not on any critical path.
        let sink = Arc::clone(&self.sink);
        let text = data.to_string();
        tokio::spawn(async move {
            let mut sink = sink.lock().await;
            let _ = sink.send(WsMessage::Text(text.into())).await;
        });
    }
}

/// Connection factory for the real KIS realtime endpoint.
pub struct TungsteniteFactory;

#[async_trait::async_trait]
impl ConnectionFactory for TungsteniteFactory {
    async fn connect(&self, url: &str) -> Result<Arc<dyn WsConnection>, String> {
        let (stream, _response) = tokio_tungstenite::connect_async(url)
            .await
            .map_err(|error| format!("ws connect to {url} failed: {error}"))?;
        let (sink, stream) = futures_util::StreamExt::split(stream);
        Ok(Arc::new(TungsteniteConnection {
            sink: Arc::new(Mutex::new(sink)),
            stream: Arc::new(Mutex::new(stream)),
        }))
    }
}

pub type QuoteCallback = Arc<dyn Fn(&Quote) + Send + Sync>;
pub type ApprovalKeyProvider =
    Arc<dyn Fn() -> BoxFuture<'static, Result<String, String>> + Send + Sync>;

// Re-export for the provider alias above.
pub use futures_util::future::BoxFuture;

/// Liveness observability for the status page: connection flag plus epoch-ms
/// timestamps of the last frame of any kind and the last market-data tick.
/// The overnight failure mode is a silent drop the TCP stack never reports
/// (measured 2026-11-24: yesterday's session went quiet through the next
/// open), so stall detection keys off silence, not socket errors.
#[derive(Default)]
pub struct WsLiveness {
    connected: AtomicBool,
    last_message_at: AtomicI64,
    last_tick_at: AtomicI64,
}

impl WsLiveness {
    pub fn connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    /// Epoch ms of the last frame of any kind (data, keepalive, system).
    pub fn last_message_at(&self) -> Option<i64> {
        let value = self.last_message_at.load(Ordering::Relaxed);
        (value > 0).then_some(value)
    }

    /// Epoch ms of the last H0STCNT0 data frame, if any.
    pub fn last_tick_at(&self) -> Option<i64> {
        let value = self.last_tick_at.load(Ordering::Relaxed);
        (value > 0).then_some(value)
    }

    fn mark_connected(&self, connected: bool) {
        self.connected.store(connected, Ordering::Relaxed);
    }

    fn mark_message(&self, at_ms: i64) {
        self.last_message_at.store(at_ms, Ordering::Relaxed);
    }

    fn mark_tick(&self, at_ms: i64) {
        self.last_tick_at.store(at_ms, Ordering::Relaxed);
    }
}

/// Streaming client with desired-state subscription replay.
pub struct KisMarketDataClient {
    approval_key: ApprovalKeyProvider,
    url: String,
    connect: Arc<dyn ConnectionFactory>,
    on_quote: QuoteCallback,
    desired: Mutex<BTreeSet<Symbol>>,
    reconnect_delays: Vec<Duration>,
    sleep: Arc<dyn Fn(Duration) -> BoxFuture<'static, ()> + Send + Sync>,
    /// Reconnect when NOTHING (data, keepalive, system message) arrives for
    /// this long — the silent-drop watchdog. Default 10 minutes.
    stale_after: Duration,
    /// Reconnect when no DATA frame arrives this long DURING the regular
    /// session — keepalives keep the socket alive, so total silence alone
    /// misses "connected but no data" (measured live 2026-11-28: PINGPONG
    /// flowed while H0STCNT0 stopped for hours). Default 3 minutes.
    tick_stall_after: Duration,
    /// Whether `now` counts as the regular session for the tick-stall
    /// watchdog. Injectable for tests; defaults to KRX hours (KST 09:00–15:30
    /// weekdays — same rule the realtime loop gates fills with).
    session_check: Arc<dyn Fn(chrono::DateTime<chrono::Utc>) -> bool + Send + Sync>,
    /// Shared liveness signals for the status page.
    state: Arc<WsLiveness>,
}

impl KisMarketDataClient {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        approval_key: ApprovalKeyProvider,
        url: String,
        connect: Arc<dyn ConnectionFactory>,
        on_quote: QuoteCallback,
    ) -> Self {
        Self {
            approval_key,
            url,
            connect,
            on_quote,
            desired: Mutex::new(BTreeSet::new()),
            reconnect_delays: vec![
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(4),
                Duration::from_secs(8),
                Duration::from_secs(16),
                Duration::from_secs(30),
            ],
            sleep: Arc::new(|duration| Box::pin(tokio::time::sleep(duration))),
            stale_after: Duration::from_secs(600),
            tick_stall_after: Duration::from_secs(180),
            session_check: Arc::new(is_regular_session_utc),
            state: Arc::new(WsLiveness::default()),
        }
    }

    /// Override the silent-drop watchdog timeout (tests + env tuning).
    pub fn with_stale_after(mut self, stale_after: Duration) -> Self {
        self.stale_after = stale_after;
        self
    }

    /// Override the market-hours data-stall watchdog timeout.
    pub fn with_tick_stall_after(mut self, tick_stall_after: Duration) -> Self {
        self.tick_stall_after = tick_stall_after;
        self
    }

    /// Override the regular-session predicate (tests inject a constant).
    pub fn with_session_check(
        mut self,
        session_check: Arc<dyn Fn(chrono::DateTime<chrono::Utc>) -> bool + Send + Sync>,
    ) -> Self {
        self.session_check = session_check;
        self
    }

    /// Shared liveness signals (connection flag, last frame/tick timestamps).
    pub fn liveness(&self) -> Arc<WsLiveness> {
        Arc::clone(&self.state)
    }

    /// Remaining time to the next stall deadline: total-silence always; plus
    /// data-frame stall when the regular-session predicate holds. The tick
    /// deadline anchors on the last tick — or, before the connection's FIRST
    /// tick, on the connection start so "subscribed but never delivered"
    /// still fires (a per-call window would reset on every keepalive).
    fn recv_timeout_until_deadline(
        &self,
        now: chrono::DateTime<chrono::Utc>,
        connection_started_ms: i64,
    ) -> Duration {
        let mut remaining = self.stale_after;
        if (self.session_check)(now) {
            let anchor = self.state.last_tick_at().unwrap_or(connection_started_ms);
            let tick_remaining = self.tick_stall_after.saturating_sub(Duration::from_millis(
                (now.timestamp_millis() - anchor).max(0) as u64,
            ));
            remaining = remaining.min(tick_remaining);
        }
        remaining
    }

    /// Which watchdog fired (log ground truth).
    fn stall_reason(
        &self,
        now: chrono::DateTime<chrono::Utc>,
        connection_started_ms: i64,
    ) -> &'static str {
        let anchor = self.state.last_tick_at().unwrap_or(connection_started_ms);
        if (self.session_check)(now)
            && now.timestamp_millis() - anchor > self.tick_stall_after.as_millis() as i64
        {
            "ws tick stall during regular session — reconnecting (data watchdog)"
        } else {
            "ws silent — reconnecting (stall watchdog)"
        }
    }

    pub async fn desired_subscriptions(&self) -> BTreeSet<Symbol> {
        self.desired.lock().await.clone()
    }

    pub async fn subscribe(&self, symbol: &Symbol) {
        self.desired.lock().await.insert(symbol.clone());
        // Live subscription (when connected) happens inside run(); callers
        // that need immediate confirmation use run()'s replay contract.
    }

    pub async fn unsubscribe(&self, symbol: &Symbol) {
        self.desired.lock().await.remove(symbol);
    }

    /// Connect, replay subscriptions, stream until cancelled.
    /// Disconnects are expected: the loop reconnects with capped backoff.
    pub async fn run(&self) {
        let mut delay_index = 0usize;
        loop {
            let connection = match self.connect.connect(&self.url).await {
                Ok(connection) => connection,
                Err(error) => {
                    eprintln!("ws connect failed ({error}) — backing off");
                    (self.sleep)(self.reconnect_delays[delay_index]).await;
                    delay_index = (delay_index + 1).min(self.reconnect_delays.len() - 1);
                    continue;
                }
            };

            let approval_key = match (self.approval_key)().await {
                Ok(key) => key,
                Err(error) => {
                    eprintln!("approval key issuance failed ({error}) — backing off");
                    (self.sleep)(self.reconnect_delays[delay_index]).await;
                    delay_index = (delay_index + 1).min(self.reconnect_delays.len() - 1);
                    continue;
                }
            };

            // Replay desired subscriptions (sorted for determinism).
            let mut replayed = 0usize;
            for symbol in self.desired.lock().await.iter() {
                let message =
                    build_subscribe_message(&approval_key, "H0STCNT0", symbol.as_str(), true);
                if connection.send(&message.to_string()).await.is_err() {
                    break;
                }
                replayed += 1;
            }
            println!(
                "ws connected to {} — {replayed} subscription(s) replayed",
                self.url
            );
            self.state.mark_connected(true);
            let connection_started_ms = chrono::Utc::now().timestamp_millis();

            delay_index = 0;
            let reason = loop {
                match tokio::time::timeout(
                    self.recv_timeout_until_deadline(chrono::Utc::now(), connection_started_ms),
                    connection.recv(),
                )
                .await
                {
                    Ok(Ok(raw)) => {
                        self.state
                            .mark_message(chrono::Utc::now().timestamp_millis());
                        self.handle_raw(&connection, &raw);
                    }
                    // recv error = disconnect: desired state replays on the next one.
                    Ok(Err(_)) => break "ws disconnected — reconnecting",
                    Err(_) => break self.stall_reason(chrono::Utc::now(), connection_started_ms),
                }
            };
            self.state.mark_connected(false);
            eprintln!("{reason}");
            (self.sleep)(self.reconnect_delays.first().copied().unwrap_or_default()).await;
        }
    }

    fn handle_raw(&self, connection: &Arc<dyn WsConnection>, raw: &str) {
        if raw.starts_with("0|") || raw.starts_with("1|") {
            static LOGGED_FULL: std::sync::atomic::AtomicBool =
                std::sync::atomic::AtomicBool::new(false);
            match parse_market_data(raw) {
                Ok(quotes) => {
                    self.state.mark_tick(chrono::Utc::now().timestamp_millis());
                    for quote in quotes {
                        (self.on_quote)(&quote);
                    }
                }
                // A real frame that fails to parse is a spec break — dump the
                // FIRST one in full (layout ground truth), then stay quiet.
                Err(error) => {
                    if !LOGGED_FULL.swap(true, std::sync::atomic::Ordering::Relaxed) {
                        eprintln!("ws frame parse failed (full frame): {error}\n{raw}");
                    }
                }
            }
            return;
        }
        if is_pingpong(raw) {
            connection.pong(raw);
            return;
        }
        // Subscription acks and realtime error notices (bad approval key,
        // no permission) arrive as these system messages — surface them.
        eprintln!("ws system message: {}", &raw[..raw.len().min(300)]);
    }
}
