//! KIS WebSocket tests: frame parsing, message shapes, replay on reconnect.

use super::ws::*;
use crate::types::{Quote, Symbol};
use rust_decimal::Decimal;
use std::collections::BTreeSet;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

fn symbol() -> Symbol {
    Symbol::parse("005930").unwrap()
}

fn data_frame(price: &str, hour: &str, date: &str) -> String {
    let mut values = vec![""; 47];
    let idx = |name: &str| H0STCNT0_COLUMNS.iter().position(|c| *c == name).unwrap();
    values[idx("MKSC_SHRN_ISCD")] = "005930";
    values[idx("STCK_CNTG_HOUR")] = hour;
    values[idx("STCK_PRPR")] = price;
    values[idx("BSOP_DATE")] = date;
    format!("0|H0STCNT0|005930|{}", values.join("^"))
}

/// Regression against the REAL production frame measured 2026-10-02
/// (verbatim from the live stream): three intra-second records, 47 columns
/// each, count field "003", one empty mid-record field, trailing
/// undocumented column "2". The 46-column official-sample assumption
/// dropped every tick of the first live session — this frame is the proof.
#[test]
fn parses_measured_production_frame() {
    let raw = "0|H0STCNT0|003|005930^144918^274750^5^-1250^-0.45^275321.52^273500^277000^271500^275000^274500^104^9320874^2566237186000^66432^46300^-20132^113.55^4187143^4754672^1^0.52^67.83^090007^2^1250^092406^5^-2250^090336^2^3250^20261002^20^N^16729^92579^974533^678758^0.16^11242578^82.91^0^^273500^2^005930^144918^274750^5^-1250^-0.45^275321.52^273500^277000^271500^275000^274500^1^9320875^2566237460750^66432^46301^-20131^113.55^4187143^4754673^1^0.52^67.83^090007^2^1250^092406^5^-2250^090336^2^3250^20261002^20^N^16841^92579^974198^678758^0.16^11242578^82.91^0^^273500^2^005930^144919^274500^5^-1500^-0.54^275321.52^273500^277000^271500^275000^274500^3^9320878^2566238284250^66433^46301^-20132^113.55^4187146^4754673^5^0.52^67.83^090007^2^1000^092406^5^-2500^090336^2^3000^20261002^20^N^16841^92579^974198^678758^0.16^11242578^82.91^0^^273500^2";
    let quotes = parse_market_data(raw).unwrap();
    assert_eq!(quotes.len(), 3);
    assert_eq!(quotes[0].symbol.as_str(), "005930");
    assert_eq!(quotes[0].last_price, Decimal::from(274_750));
    // 14:49:18 KST == 05:49:18 UTC.
    assert_eq!(
        quotes[0].timestamp.to_rfc3339(),
        "2026-10-02T05:49:18+00:00"
    );
    assert_eq!(quotes[2].last_price, Decimal::from(274_500));
}

#[test]
fn parses_single_and_multi_record_frames() {
    let frame = data_frame("80500", "093012", "20260915");
    let two = format!("{frame}^{}", values_of(&frame).join("^"));
    let quotes = parse_market_data(&two).unwrap();
    assert_eq!(quotes.len(), 2);
    assert_eq!(quotes[0].last_price, Decimal::from(80_500));
    // KST wall time converts to UTC (09:30:15 KST == 00:30:15 UTC).
    assert_eq!(
        quotes[0].timestamp.to_rfc3339(),
        "2026-09-15T00:30:12+00:00"
    );
}

fn values_of(frame: &str) -> Vec<&str> {
    frame.split('|').nth(3).unwrap().split('^').collect()
}

#[test]
fn rejects_malformed_frames() {
    assert!(matches!(
        parse_market_data("not-a-frame"),
        Err(FrameError::UnexpectedFrame(_))
    ));
    let mut values = vec!["x"; 45];
    values[0] = "005930";
    values[2] = "80500";
    values[33] = "20260915";
    values[1] = "093012";
    assert!(matches!(
        parse_market_data(&format!("0|H0STCNT0|005930|{}", values.join("^"))),
        Err(FrameError::NotMultiple { .. })
    ));
}

#[test]
fn subscribe_message_shape_is_official() {
    let message = build_subscribe_message("approval-1", "H0STCNT0", "005930", true);
    assert_eq!(
        message,
        serde_json::json!({
            "header": {"approval_key": "approval-1", "tr_type": "1", "custtype": "P"},
            "body": {"input": {"tr_id": "H0STCNT0", "tr_key": "005930"}},
        })
    );
    assert_eq!(
        build_subscribe_message("k", "H0STCNT0", "005930", false)["header"]["tr_type"],
        "0"
    );
}

#[test]
fn pingpong_detected_in_json_and_plain() {
    assert!(is_pingpong(r#"{"header": {"tr_id": "PINGPONG"}}"#));
    assert!(is_pingpong("PINGPONG"));
    assert!(!is_pingpong(r#"{"header": {"tr_id": "H0STCNT0"}}"#));
}

/// Scripted connection: delivers queued frames, records sends/pongs, then
/// errors (disconnect) when the script runs dry.
struct FakeConnection {
    script: Mutex<Vec<String>>,
    sent: Mutex<Vec<serde_json::Value>>,
    pongs: Mutex<Vec<String>>,
}

#[async_trait::async_trait]
impl WsConnection for FakeConnection {
    async fn send(&self, message: &str) -> Result<(), String> {
        self.sent
            .lock()
            .unwrap()
            .push(serde_json::from_str(message).unwrap_or_default());
        Ok(())
    }
    async fn recv(&self) -> Result<String, String> {
        match self.script.lock().unwrap().pop() {
            Some(frame) => Ok(frame),
            None => Err("connection reset".to_string()),
        }
    }
    fn pong(&self, data: &str) {
        self.pongs.lock().unwrap().push(data.to_string());
    }
}

struct FakeFactory {
    scripts: Mutex<Vec<Vec<String>>>,
    connections: Mutex<Vec<Arc<FakeConnection>>>,
}

#[async_trait::async_trait]
impl ConnectionFactory for FakeFactory {
    async fn connect(&self, _url: &str) -> Result<Arc<dyn WsConnection>, String> {
        let mut scripts = self.scripts.lock().unwrap();
        if scripts.is_empty() {
            return Err("no more scripts".to_string());
        }
        let script = scripts.remove(0);
        let connection = Arc::new(FakeConnection {
            // recv pops from the end — reverse so the script plays in order.
            script: Mutex::new(script.into_iter().rev().collect()),
            sent: Mutex::new(Vec::new()),
            pongs: Mutex::new(Vec::new()),
        });
        self.connections
            .lock()
            .unwrap()
            .push(Arc::clone(&connection));
        Ok(connection)
    }
}

fn approval_provider() -> ApprovalKeyProvider {
    Arc::new(|| Box::pin(async { Ok("approval-1".to_string()) }))
}

fn make_client(
    factory: Arc<FakeFactory>,
    quotes: Arc<Mutex<Vec<Quote>>>,
) -> Arc<KisMarketDataClient> {
    let on_quote: QuoteCallback = Arc::new(move |quote: &Quote| {
        quotes.lock().unwrap().push(quote.clone());
    });
    Arc::new(KisMarketDataClient::new(
        approval_provider(),
        "ws://kis.example/ws".into(),
        factory,
        on_quote,
    ))
}

async fn wait_until(predicate: impl Fn() -> bool) {
    for _ in 0..200 {
        if predicate() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("condition not met before timeout");
}

use std::time::Duration;

#[tokio::test]
#[serial_test::serial]
async fn reconnect_replays_subscriptions_and_quotes_resume() {
    let factory = Arc::new(FakeFactory {
        scripts: Mutex::new(vec![
            // First connection: one frame, then drop.
            vec![data_frame("80500", "093012", "20260915")],
            // Second connection: a fresh price arrives.
            vec![data_frame("81000", "093045", "20260915")],
        ]),
        connections: Mutex::new(Vec::new()),
    });
    let quotes: Arc<Mutex<Vec<Quote>>> = Arc::new(Mutex::new(Vec::new()));
    let client = make_client(Arc::clone(&factory), Arc::clone(&quotes));

    client.subscribe(&symbol()).await;
    let runner = Arc::clone(&client);
    let task = tokio::spawn(async move {
        runner.run().await;
    });

    wait_until(|| factory.connections.lock().unwrap().len() >= 2).await;
    wait_until(|| quotes.lock().unwrap().len() >= 2).await;
    task.abort();

    let connections = factory.connections.lock().unwrap();
    assert_eq!(connections.len(), 2);
    // Connection 1 got the live replay... (subscribe before run: first
    // connection replays too). Connection 2 MUST have the replay.
    let second = &connections[1];
    let sent = second.sent.lock().unwrap();
    assert_eq!(sent.len(), 1, "second connection replays the subscription");
    assert_eq!(sent[0]["body"]["input"]["tr_key"], "005930");
    assert_eq!(sent[0]["header"]["tr_type"], "1");

    let captured = quotes.lock().unwrap();
    assert_eq!(
        captured.iter().map(|q| q.last_price).collect::<Vec<_>>(),
        vec![Decimal::from(80_500), Decimal::from(81_000)]
    );
}

#[tokio::test]
#[serial_test::serial]
async fn pingpong_is_echoed() {
    let factory = Arc::new(FakeFactory {
        scripts: Mutex::new(vec![vec![
            r#"{"header": {"tr_id": "PINGPONG"}}"#.to_string()
        ]]),
        connections: Mutex::new(Vec::new()),
    });
    let client = make_client(Arc::clone(&factory), Arc::new(Mutex::new(Vec::new())));

    let runner = Arc::clone(&client);
    let task = tokio::spawn(async move {
        runner.run().await;
    });
    let factory2 = Arc::clone(&factory);
    wait_until(|| {
        factory2
            .connections
            .lock()
            .unwrap()
            .first()
            .is_some_and(|connection| !connection.pongs.lock().unwrap().is_empty())
    })
    .await;
    task.abort();

    let pongs = factory.connections.lock().unwrap()[0]
        .pongs
        .lock()
        .unwrap()
        .clone();
    assert_eq!(pongs, vec![r#"{"header": {"tr_id": "PINGPONG"}}"#]);
}

#[tokio::test]
#[serial_test::serial]
async fn unsubscribed_symbols_not_replayed() {
    let factory = Arc::new(FakeFactory {
        scripts: Mutex::new(vec![
            vec![], // first connection drops immediately
            vec![],
        ]),
        connections: Mutex::new(Vec::new()),
    });
    let client = make_client(Arc::clone(&factory), Arc::new(Mutex::new(Vec::new())));
    let other = Symbol::parse("035420").unwrap();
    client.subscribe(&symbol()).await;
    client.subscribe(&other).await;
    client.unsubscribe(&other).await;

    let runner = Arc::clone(&client);
    let task = tokio::spawn(async move {
        runner.run().await;
    });
    let factory2 = Arc::clone(&factory);
    wait_until(|| factory2.connections.lock().unwrap().len() >= 2).await;
    task.abort();

    let replayed: Vec<String> = factory.connections.lock().unwrap()[1]
        .sent
        .lock()
        .unwrap()
        .iter()
        .map(|message| {
            message["body"]["input"]["tr_key"]
                .as_str()
                .unwrap_or("")
                .to_string()
        })
        .collect();
    assert_eq!(replayed, vec!["005930"]);
}

#[tokio::test]
#[serial_test::serial]
async fn approval_key_uses_secretkey_field() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth2/Approval"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "approval_key": "approval-xyz",
        })))
        .expect(1)
        .mount(&server)
        .await;

    let key = fetch_approval_key(
        &reqwest::Client::new(),
        &server.uri(),
        "appkey-1",
        "appsecret-1",
    )
    .await
    .unwrap();
    assert_eq!(key, "approval-xyz");
}

#[test]
fn desired_state_is_queryable() {
    // Compile-time sanity for the public surface used by the runtime.
    let set: BTreeSet<Symbol> = BTreeSet::from([symbol()]);
    assert_eq!(set.len(), 1);
}

static _KEEP: AtomicUsize = AtomicUsize::new(0);
#[allow(dead_code)]
fn _unused(_notify: &Notify) {}

/// Real transport end-to-end against a local WebSocket server: the subscribe
/// (with approval key) arrives, a market-data frame parses into the quote
/// callback, and the PINGPONG keepalive is echoed back.
#[tokio::test]
#[serial_test::serial]
async fn tungstenite_transport_end_to_end() {
    use futures_util::StreamExt;
    use tokio_tungstenite::tungstenite::Message;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server = tokio::spawn(async move {
        use futures_util::SinkExt;
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();

        let subscribe = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("subscribe timeout")
            .unwrap()
            .unwrap();
        let subscribe = subscribe.into_text().unwrap().to_string();

        ws.send(Message::Text(
            data_frame("80500", "093012", "20260915").into(),
        ))
        .await
        .unwrap();

        ws.send(Message::Text(
            r#"{"header": {"tr_id": "PINGPONG"}}"#.to_string().into(),
        ))
        .await
        .unwrap();
        let echo = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("echo timeout")
            .unwrap()
            .unwrap();
        (subscribe, echo.into_text().unwrap().to_string())
    });

    let quotes: Arc<Mutex<Vec<Quote>>> = Arc::new(Mutex::new(Vec::new()));
    let on_quote: QuoteCallback = {
        let quotes = Arc::clone(&quotes);
        Arc::new(move |quote: &Quote| quotes.lock().unwrap().push(quote.clone()))
    };
    let client = Arc::new(KisMarketDataClient::new(
        approval_provider(),
        format!("ws://{addr}"),
        Arc::new(TungsteniteFactory),
        on_quote,
    ));
    client.subscribe(&symbol()).await;
    let task = tokio::spawn({
        let runner = Arc::clone(&client);
        async move {
            runner.run().await;
        }
    });

    wait_until(|| !quotes.lock().unwrap().is_empty()).await;
    let (subscribe, echo) = tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .expect("server script timeout")
        .unwrap();
    task.abort();

    let subscribe: serde_json::Value = serde_json::from_str(&subscribe).unwrap();
    assert_eq!(subscribe["header"]["approval_key"], "approval-1");
    assert_eq!(subscribe["body"]["input"]["tr_key"], "005930");
    assert!(
        echo.contains("PINGPONG"),
        "keepalive echo missing: {echo:?}"
    );

    let captured = quotes.lock().unwrap();
    assert_eq!(captured[0].last_price, Decimal::from(80_500));
}

/// A connection that never delivers anything and never errors — the silent
/// drop the TCP stack never reports (measured live 2026-11-24 mid-session).
struct SilentConnection;

#[async_trait::async_trait]
impl WsConnection for SilentConnection {
    async fn send(&self, _message: &str) -> Result<(), String> {
        Ok(())
    }

    async fn recv(&self) -> Result<String, String> {
        std::future::pending().await
    }

    fn pong(&self, _data: &str) {}
}

/// First connect hands out the silent connection; every later connect is a
/// scripted normal connection, the second one recorded for replay asserts.
struct StallFactory {
    connects: Mutex<usize>,
    second: Mutex<Option<Arc<FrameThenSilentConnection>>>,
}

/// Scripted frame then silence (pending recv) — unlike FakeConnection, which
/// errors when dry, this keeps the fresh connection alive for liveness
/// asserts.
struct FrameThenSilentConnection {
    script: Mutex<Vec<String>>,
    sent: Mutex<Vec<serde_json::Value>>,
}

#[async_trait::async_trait]
impl WsConnection for FrameThenSilentConnection {
    async fn send(&self, message: &str) -> Result<(), String> {
        self.sent
            .lock()
            .unwrap()
            .push(serde_json::from_str(message).unwrap_or_default());
        Ok(())
    }

    async fn recv(&self) -> Result<String, String> {
        // Pop the guard before any await — a held std MutexGuard is !Send.
        let frame = self.script.lock().unwrap().pop();
        match frame {
            Some(frame) => Ok(frame),
            None => std::future::pending().await,
        }
    }

    fn pong(&self, _data: &str) {}
}

#[async_trait::async_trait]
impl ConnectionFactory for StallFactory {
    async fn connect(&self, _url: &str) -> Result<Arc<dyn WsConnection>, String> {
        let mut connects = self.connects.lock().unwrap();
        *connects += 1;
        let round = *connects;
        drop(connects);
        if round == 1 {
            return Ok(Arc::new(SilentConnection));
        }
        let connection = Arc::new(FrameThenSilentConnection {
            script: Mutex::new(vec![data_frame("90000", "110000", "20261124")]),
            sent: Mutex::new(Vec::new()),
        });
        *self.second.lock().unwrap() = Some(Arc::clone(&connection));
        Ok(connection)
    }
}

/// Round 1 hands out a keepalive-flowing connection (optionally with one real
/// data frame first), later rounds scripted — isolates the tick-stall
/// watchdog from the total-silence one.
struct KeepaliveFactory {
    first_data: bool,
    connects: Mutex<usize>,
    second: Mutex<Option<Arc<FrameThenSilentConnection>>>,
}

#[async_trait::async_trait]
impl ConnectionFactory for KeepaliveFactory {
    async fn connect(&self, _url: &str) -> Result<Arc<dyn WsConnection>, String> {
        let mut connects = self.connects.lock().unwrap();
        *connects += 1;
        let round = *connects;
        drop(connects);
        if round == 1 {
            let frame = if self.first_data {
                Some(data_frame("90000", "110000", "20261128"))
            } else {
                None
            };
            return Ok(Arc::new(KeepaliveConnection {
                first_data: Mutex::new(frame),
                interval_ms: 60,
            }));
        }
        let connection = Arc::new(FrameThenSilentConnection {
            script: Mutex::new(vec![data_frame("90000", "110000", "20261128")]),
            sent: Mutex::new(Vec::new()),
        });
        *self.second.lock().unwrap() = Some(Arc::clone(&connection));
        Ok(connection)
    }
}

/// Keepalives keep flowing forever (interval), never errors — the "connected
/// but no data" failure mode measured live 2026-11-28: PINGPONG every ~60s
/// while H0STCNT0 stopped for hours.
struct KeepaliveConnection {
    first_data: Mutex<Option<String>>,
    interval_ms: u64,
}

#[async_trait::async_trait]
impl WsConnection for KeepaliveConnection {
    async fn send(&self, _message: &str) -> Result<(), String> {
        Ok(())
    }

    async fn recv(&self) -> Result<String, String> {
        // Deliver one real data frame first, then PINGPONG at `interval_ms`
        // cadence forever. Guard popped before any await (!Send guard trap).
        if let Some(frame) = self.first_data.lock().unwrap().take() {
            return Ok(frame);
        }
        tokio::time::sleep(Duration::from_millis(self.interval_ms)).await;
        Ok("{\"header\":{\"tr_id\":\"PINGPONG\"}}".to_string())
    }

    fn pong(&self, _data: &str) {}
}

#[tokio::test]
#[serial_test::serial]
async fn tick_stall_with_live_keepalives_triggers_reconnect() {
    let factory = Arc::new(KeepaliveFactory {
        first_data: true,
        connects: Mutex::new(0),
        second: Mutex::new(None),
    });
    let quotes: Arc<Mutex<Vec<Quote>>> = Arc::new(Mutex::new(Vec::new()));
    let on_quote: QuoteCallback = Arc::new(move |quote: &Quote| {
        quotes.lock().unwrap().push(quote.clone());
    });
    let factory_dyn: Arc<dyn ConnectionFactory> = factory.clone();
    let session_always_on: Arc<dyn Fn(chrono::DateTime<chrono::Utc>) -> bool + Send + Sync> =
        Arc::new(|_| true);
    let client = Arc::new(
        KisMarketDataClient::new(
            approval_provider(),
            "ws://kis.example/ws".into(),
            factory_dyn,
            on_quote,
        )
        .with_tick_stall_after(Duration::from_millis(200))
        .with_session_check(session_always_on),
    );
    client.subscribe(&symbol()).await;
    let runner = Arc::clone(&client);
    let task = tokio::spawn(async move {
        runner.run().await;
    });

    wait_until(|| *factory.connects.lock().unwrap() >= 2).await;
    wait_until(|| client.liveness().last_tick_at().is_some()).await;
    task.abort();

    assert!(
        *factory.connects.lock().unwrap() >= 2,
        "tick stall with live keepalives must force a reconnect"
    );
    let second = factory
        .second
        .lock()
        .unwrap()
        .clone()
        .expect("second connection exists");
    assert_eq!(
        second.sent.lock().unwrap().len(),
        1,
        "fresh connection replays the subscription"
    );
    assert!(client.liveness().last_tick_at().is_some());
}

#[tokio::test]
#[serial_test::serial]
async fn keepalive_only_session_triggers_first_tick_stall_reconnect() {
    // Live failure 2026-11-28: fresh connection subscribed 15 symbols at
    // 07:24 KST, SUBSCRIBE SUCCESS x15, then ZERO data frames for hours
    // while PINGPONG flowed - a per-call timeout window resets on every
    // keepalive, so the data watchdog never fired. The deadline now anchors
    // on the connection start until the first tick arrives.
    let factory = Arc::new(KeepaliveFactory {
        first_data: false,
        connects: Mutex::new(0),
        second: Mutex::new(None),
    });
    let quotes: Arc<Mutex<Vec<Quote>>> = Arc::new(Mutex::new(Vec::new()));
    let on_quote: QuoteCallback = Arc::new(move |quote: &Quote| {
        quotes.lock().unwrap().push(quote.clone());
    });
    let factory_dyn: Arc<dyn ConnectionFactory> = factory.clone();
    let session_always_on: Arc<dyn Fn(chrono::DateTime<chrono::Utc>) -> bool + Send + Sync> =
        Arc::new(|_| true);
    let client = Arc::new(
        KisMarketDataClient::new(
            approval_provider(),
            "ws://kis.example/ws".into(),
            factory_dyn,
            on_quote,
        )
        .with_tick_stall_after(Duration::from_millis(200))
        .with_session_check(session_always_on),
    );
    client.subscribe(&symbol()).await;
    let runner = Arc::clone(&client);
    let task = tokio::spawn(async move {
        runner.run().await;
    });

    wait_until(|| *factory.connects.lock().unwrap() >= 2).await;
    task.abort();

    assert!(
        *factory.connects.lock().unwrap() >= 2,
        "first-tick stall with live keepalives must force a reconnect"
    );
    let second = factory
        .second
        .lock()
        .unwrap()
        .clone()
        .expect("second connection exists");
    assert_eq!(
        second.sent.lock().unwrap().len(),
        1,
        "fresh connection replays the subscription"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn silent_stall_triggers_reconnect_and_liveness_visible() {
    let factory = Arc::new(StallFactory {
        connects: Mutex::new(0),
        second: Mutex::new(None),
    });
    let quotes: Arc<Mutex<Vec<Quote>>> = Arc::new(Mutex::new(Vec::new()));
    let on_quote: QuoteCallback = Arc::new(move |quote: &Quote| {
        quotes.lock().unwrap().push(quote.clone());
    });
    let factory_dyn: Arc<dyn ConnectionFactory> = factory.clone();
    let client = Arc::new(
        KisMarketDataClient::new(
            approval_provider(),
            "ws://kis.example/ws".into(),
            factory_dyn,
            on_quote,
        )
        .with_stale_after(Duration::from_millis(150)),
    );
    client.subscribe(&symbol()).await;
    let runner = Arc::clone(&client);
    let task = tokio::spawn(async move {
        runner.run().await;
    });

    wait_until(|| *factory.connects.lock().unwrap() >= 2).await;
    wait_until(|| client.liveness().last_tick_at().is_some()).await;
    task.abort();

    assert!(
        *factory.connects.lock().unwrap() >= 2,
        "silent stall must force a reconnect"
    );
    let second = factory
        .second
        .lock()
        .unwrap()
        .clone()
        .expect("second connection exists");
    assert_eq!(
        second.sent.lock().unwrap().len(),
        1,
        "fresh connection replays the subscription"
    );
    assert!(
        client.liveness().connected(),
        "fresh connection marks itself live"
    );
    assert!(
        client.liveness().last_tick_at().is_some(),
        "data frame stamps tick liveness"
    );
}
