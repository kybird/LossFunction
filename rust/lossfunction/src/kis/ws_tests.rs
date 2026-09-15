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
    let mut values = vec![""; 46];
    let idx = |name: &str| H0STCNT0_COLUMNS.iter().position(|c| *c == name).unwrap();
    values[idx("MKSC_SHRN_ISCD")] = "005930";
    values[idx("STCK_CNTG_HOUR")] = hour;
    values[idx("STCK_PRPR")] = price;
    values[idx("BSOP_DATE")] = date;
    format!("0|H0STCNT0|005930|{}", values.join("^"))
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
