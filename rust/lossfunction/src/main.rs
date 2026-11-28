//! Container entrypoint — health, status page, controls, demo loop.
//!
//! Paper by default; live without the double confirmation refuses to boot
//! (crash by design that the container restart policy then surfaces). The
//! KIS-backed brokers land with the KIS client cards — until then this
//! entrypoint supports the in-memory paper backend (optionally with the
//! synthetic demo market).

use std::sync::Arc;

use lossfunction::broker::mock::MockBroker;
use lossfunction::config::{PaperBackend, Settings};
use lossfunction::risk::{RiskLimits, RiskManager};
use lossfunction::runtime::demo::DemoLoop;
use lossfunction::runtime::realtime::RealtimeLoop;
use lossfunction::runtime::server::{AppState, SharedState};
use lossfunction::runtime::sim::SimLoop;
use lossfunction::storage::Repository;

/// One-shot backtest baseline: walk-forward every registered strategy over
/// stored candles and print each summary (also persisted as suggestions).
async fn optimize_all_strategies(settings: &Settings, symbols_csv: Option<String>) {
    use lossfunction::strategy_registry;
    use lossfunction::types::Symbol;

    let repository = Repository::open(&settings.database_path)
        .await
        .expect("open sqlite database");
    let symbols: Vec<Symbol> = match symbols_csv {
        Some(csv) => csv
            .split(',')
            .map(|code| code.trim())
            .filter(|code| !code.is_empty())
            .map(|code| Symbol::parse(code).expect("valid symbol code"))
            .collect(),
        None => repository
            .watchlist()
            .await
            .ok()
            .filter(|list| !list.is_empty())
            .unwrap_or_else(|| settings.watchlist.clone()),
    };
    println!(
        "optimize: {} strategies x {} symbols (walk-forward 70/30, survivors 3)",
        strategy_registry::registry().len(),
        symbols.len()
    );
    for spec in strategy_registry::registry() {
        match lossfunction::runtime::optimize_runner::run_optimize(
            &repository,
            &repository,
            spec.key,
            &symbols,
            5,
        )
        .await
        {
            Ok(summary) => println!("[{}] {}", spec.key, summary),
            Err(error) => println!("[{}] FAILED — {}", spec.key, error),
        }
    }
}

/// Per-symbol × per-strategy validation-return matrix (DEFAULT params only).
async fn matrix_per_symbol(settings: &Settings, symbols_csv: Option<String>) {
    use lossfunction::backtest::{load_bars_from_candles, BacktestConfig};
    use lossfunction::backtest_optimize::walk_forward;
    use lossfunction::strategy_registry;
    use lossfunction::types::Symbol;

    let repository = Repository::open(&settings.database_path)
        .await
        .expect("open sqlite database");
    let symbols: Vec<Symbol> = match symbols_csv {
        Some(csv) => csv
            .split(',')
            .map(|code| code.trim())
            .filter(|code| !code.is_empty())
            .map(|code| Symbol::parse(code).expect("valid symbol code"))
            .collect(),
        None => repository
            .watchlist()
            .await
            .ok()
            .filter(|list| !list.is_empty())
            .unwrap_or_else(|| settings.watchlist.clone()),
    };
    let strategies: Vec<_> = strategy_registry::registry();
    println!(
        "matrix: {} symbols x {} strategies (walk-forward 70/30, DEFAULT params only)",
        symbols.len(),
        strategies.len()
    );
    print!("{:<8}", "symbol");
    for spec in &strategies {
        print!("{:>12}", spec.key);
    }
    println!();
    for symbol in &symbols {
        let one = [symbol.clone()];
        let bars = match load_bars_from_candles(&repository, &one, 120).await {
            Ok(bars) => bars,
            Err(error) => {
                println!("{:<8} SKIP — {error}", symbol.as_str());
                continue;
            }
        };
        print!("{:<8}", symbol.as_str());
        for spec in &strategies {
            let cell = match walk_forward(&bars, spec.key, &one, BacktestConfig::default(), 0.7, 1)
                .await
            {
                Ok(outcomes) => {
                    let default = outcomes
                        .iter()
                        .find(|outcome| outcome.is_default)
                        .expect("default outcome present");
                    format!("{:>9.2}%", default.validation.return_pct)
                }
                Err(_) => "         —".to_string(),
            };
            print!("{cell:>12}");
        }
        println!();
    }
}

async fn backfill_daily_bars(settings: &Settings, years: i64) {
    use lossfunction::kis::auth::{kis_base_url, KisAuth};
    use lossfunction::kis::chart::KisChartSource;
    use lossfunction::kis::rest::KisRestClient;
    use lossfunction::marketdata::backfill_daily;

    if settings.kis_app_key.expose().is_empty() || settings.kis_app_secret.expose().is_empty() {
        eprintln!("backfill needs KIS_APP_KEY / KIS_APP_SECRET in the environment");
        std::process::exit(2);
    }
    let environment = match settings.kis_environment {
        lossfunction::config::KisEnvironment::Real => "real",
        lossfunction::config::KisEnvironment::Mock => "mock",
    };
    let base_url = kis_base_url(environment).to_string();
    let http = reqwest::Client::new();
    let auth = KisAuth::new(
        base_url.clone(),
        settings.kis_app_key.expose().to_string(),
        settings.kis_app_secret.expose().to_string(),
        http.clone(),
    );
    let rest = KisRestClient::new(
        auth,
        &settings.kis_account_number,
        environment,
        base_url,
        http,
        "backfill",
    )
    .expect("valid account number for the chart client");
    let source = KisChartSource::new(rest);
    let repository = Repository::open(&settings.database_path)
        .await
        .expect("open sqlite database");
    repository.migrate().await.expect("run migrations");
    repository
        .seed_watchlist(&settings.watchlist)
        .await
        .expect("seed watchlist");
    repository.migrate().await.expect("run migrations");

    let to = chrono::Utc::now();
    let from = to - chrono::Duration::days(365 * years);
    println!(
        "backfill: {} symbols x {}y -> {} (domain: {})",
        settings.watchlist.len(),
        years,
        settings.database_path,
        environment
    );
    let mut outcomes = backfill_daily(&source, &repository, &settings.watchlist, from, to).await;

    // KIS serves short-term 500s under load (raw 2026-09-20 Case 9 — cleared
    // after a 90s pause). Retry server-side failures once; the token is
    // cached in-process, so no tokenP re-issue (once-a-day principle).
    let server_failed: Vec<lossfunction::types::Symbol> = outcomes
        .iter()
        .filter(|(_, outcome)| {
            matches!(
                outcome,
                Err(lossfunction::marketdata::MarketDataError::Source(message))
                    if message.contains("server error")
            )
        })
        .map(|(symbol, _)| symbol.clone())
        .collect();
    if !server_failed.is_empty() {
        println!(
            "  {} symbol(s) hit KIS server 500s — retrying once after 90s (token reused)",
            server_failed.len()
        );
        tokio::time::sleep(std::time::Duration::from_secs(90)).await;
        for (symbol, outcome) in
            backfill_daily(&source, &repository, &server_failed, from, to).await
        {
            if let Some(slot) = outcomes
                .iter_mut()
                .find(|(existing, _)| existing == &symbol)
            {
                slot.1 = outcome;
            }
        }
    }

    let mut failures = 0usize;
    for (symbol, outcome) in &outcomes {
        match outcome {
            Ok(count) => println!("  {symbol}: {count} bars stored"),
            Err(error) => {
                failures += 1;
                println!("  {symbol}: FAILED — {error}");
            }
        }
    }
    let total: u64 = outcomes
        .iter()
        .filter_map(|(_, o)| o.as_ref().ok().copied())
        .sum();
    println!(
        "done: {total} bars, {failures}/{} symbols failed",
        outcomes.len()
    );
    if failures > 0 {
        std::process::exit(1);
    }
}

#[tokio::main]
async fn main() {
    // Container HEALTHCHECK probe: exit 0 when the local endpoint answers.
    if std::env::args().any(|arg| arg == "--healthcheck") {
        let port: u16 = std::env::var("HEALTH_PORT")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(8080);
        let url = format!("http://127.0.0.1:{port}/healthz");
        match reqwest::get(url).await {
            Ok(response) if response.status().is_success() => std::process::exit(0),
            _ => std::process::exit(1),
        }
    }

    let settings = Settings::load().unwrap_or_else(|error| {
        eprintln!("settings refused to load: {error}");
        std::process::exit(2);
    });

    // Read-only daily-bar backfill from the KIS chart API into candles.
    // Quotation endpoints only — no order can leave through this path.
    // `--backfill-daily [years]` (default 5).
    if let Some(position) = std::env::args().position(|arg| arg == "--backfill-daily") {
        let years: i64 = std::env::args()
            .nth(position + 1)
            .and_then(|value| value.parse().ok())
            .unwrap_or(5);
        backfill_daily_bars(&settings, years).await;
        return;
    }

    // Headless walk-forward over every registered strategy — the backtest
    // half of the backtest → paper → live ladder. `--optimize-all [symbols]`
    // (comma-separated codes; default: the stored watchlist).
    if let Some(position) = std::env::args().position(|arg| arg == "--optimize-all") {
        let symbols_csv = std::env::args()
            .nth(position + 1)
            .filter(|value| !value.starts_with('-'));
        optimize_all_strategies(&settings, symbols_csv).await;
        return;
    }

    // Per-symbol strategy fit matrix: every registered strategy's DEFAULT
    // parameters, walk-forward validation return, one symbol at a time.
    // No per-symbol param selection — that would be a multiple-comparison
    // explosion (raw 2026-11-21 Case 3); patterns, not per-symbol winners.
    // `--matrix-per-symbol [symbols]` (default: stored watchlist).
    if let Some(position) = std::env::args().position(|arg| arg == "--matrix-per-symbol") {
        let symbols_csv = std::env::args()
            .nth(position + 1)
            .filter(|value| !value.starts_with('-'));
        matrix_per_symbol(&settings, symbols_csv).await;
        return;
    }
    let port: u16 = std::env::var("HEALTH_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(8080);

    let broker: Arc<MockBroker> = match (
        settings.paper_backend,
        settings.kis_environment,
        settings.trading_mode,
    ) {
        (PaperBackend::Memory, _, _) => Arc::new(MockBroker::new()),
        // KIS-backed wiring lands with the KIS client cards; refuse loudly
        // instead of silently falling back to paper.
        _ => {
            eprintln!(
                "backend {} is not wired in the Rust runtime yet (KIS client cards pending); \
                 refusing to start — use PAPER_BACKEND=memory",
                match settings.paper_backend {
                    PaperBackend::Kis => "kis",
                    PaperBackend::Memory => unreachable!(),
                }
            );
            std::process::exit(2);
        }
    };

    let simulation = std::env::var("SIMULATION")
        .map(|value| value == "true")
        .unwrap_or(false);
    let mut risk_limits = RiskLimits::from(settings.risk);
    // Replay feeds historical timestamps; the production 30s staleness gate
    // would reject every simulated order as stale market data.
    if simulation {
        risk_limits.stale_quote_max_age = chrono::TimeDelta::weeks(520);
    }
    let risk = Arc::new(RiskManager::new(risk_limits));
    let repository = Repository::open(&settings.database_path)
        .await
        .expect("open sqlite database");

    let backfill_creds = if settings.kis_app_key.expose().is_empty()
        || settings.kis_app_secret.expose().is_empty()
    {
        None
    } else {
        Some(lossfunction::runtime::backfill::BackfillCredentials {
            environment: match settings.kis_environment {
                lossfunction::config::KisEnvironment::Real => "real",
                lossfunction::config::KisEnvironment::Mock => "mock",
            }
            .to_string(),
            app_key: settings.kis_app_key.expose().to_string(),
            app_secret: settings.kis_app_secret.expose().to_string(),
            account: settings.kis_account_number.clone(),
            base_url_override: None,
        })
    };
    // Simulation replay: real stored candles through the live pipeline with
    // the mock broker only — paper trading on real data, no venue contact.
    let simulation = std::env::var("SIMULATION")
        .map(|value| value == "true")
        .unwrap_or(false);
    let sim_strategy = std::env::var("SIM_STRATEGY").unwrap_or_else(|_| "sma-cross".to_string());

    // Candle source for lab controls (backtest/optimize): the backfilled
    // real bars live in SIM_SOURCE during simulation sessions; otherwise
    // the main database is the source.
    let source_path =
        std::env::var("SIM_SOURCE").unwrap_or_else(|_| settings.database_path.clone());
    let source = Repository::open(&source_path)
        .await
        .expect("open simulation source database");
    source.migrate().await.expect("migrate simulation source");

    let mut tasks = Vec::new();
    let mut ws_liveness: Option<Arc<lossfunction::kis::ws::WsLiveness>> = None;
    let strategy_label = if simulation {
        let sim = SimLoop::new(
            Arc::clone(&broker),
            Arc::clone(&risk),
            demo_repository(&settings).await,
            source.clone(),
            settings.watchlist.clone(),
            &sim_strategy,
            std::time::Duration::from_millis(300),
        )
        .await;
        let label = sim.strategy_label();
        println!(
            "simulation replay enabled: {} bars x {} (strategy {}, mock fills only)",
            0,
            settings.watchlist.len(),
            sim_strategy
        );
        tasks.push(tokio::spawn(sim_run(sim)));
        label
    } else if settings.quotes_source == lossfunction::config::QuotesSource::Kis {
        // Realtime fake-fill simulation: KIS realtime quotes (read-only
        // market data) drive the mock broker — no order can leave.
        let environment = match settings.kis_environment {
            lossfunction::config::KisEnvironment::Real => "real",
            lossfunction::config::KisEnvironment::Mock => "mock",
        };
        let http = reqwest::Client::new();
        let rest_base = lossfunction::kis::auth::kis_base_url(environment).to_string();
        let ws_url = lossfunction::kis::auth::kis_ws_url(environment).to_string();
        let provider = {
            let http = http.clone();
            let rest_base = rest_base.clone();
            let app_key = settings.kis_app_key.expose().to_string();
            let app_secret = settings.kis_app_secret.expose().to_string();
            Arc::new(move || {
                let http = http.clone();
                let rest_base = rest_base.clone();
                let app_key = app_key.clone();
                let app_secret = app_secret.clone();
                Box::pin(async move {
                    lossfunction::kis::ws::fetch_approval_key(
                        &http,
                        &rest_base,
                        &app_key,
                        &app_secret,
                    )
                    .await
                })
                    as lossfunction::kis::ws::BoxFuture<'static, Result<String, String>>
            }) as lossfunction::kis::ws::ApprovalKeyProvider
        };
        let (quote_tx, quote_rx) = tokio::sync::mpsc::unbounded_channel();
        let on_quote = {
            let quote_tx = quote_tx.clone();
            // First tick per symbol — the "data is actually flowing" signal.
            let seen: std::sync::Mutex<std::collections::HashSet<String>> =
                std::sync::Mutex::new(std::collections::HashSet::new());
            Arc::new(move |quote: &lossfunction::types::Quote| {
                if seen
                    .lock()
                    .unwrap()
                    .insert(quote.symbol.as_str().to_string())
                {
                    println!(
                        "first live tick: {} @ {}",
                        quote.symbol.as_str(),
                        quote.last_price.round_dp(0)
                    );
                }
                // Receiver gone = loop task ended; drop the tick.
                let _ = quote_tx.send(quote.clone());
            }) as lossfunction::kis::ws::QuoteCallback
        };
        let stale_secs: u64 = std::env::var("REALTIME_WS_STALE_SECS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(600);
        let tick_stall_secs: u64 = std::env::var("REALTIME_TICK_STALL_SECS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(180);
        let client = Arc::new(
            lossfunction::kis::ws::KisMarketDataClient::new(
                provider,
                ws_url,
                Arc::new(lossfunction::kis::ws::TungsteniteFactory),
                on_quote,
            )
            .with_stale_after(std::time::Duration::from_secs(stale_secs))
            .with_tick_stall_after(std::time::Duration::from_secs(tick_stall_secs)),
        );
        ws_liveness = Some(client.liveness());
        for symbol in &settings.watchlist {
            client.subscribe(symbol).await;
        }
        let strategy_key =
            std::env::var("REALTIME_STRATEGY").unwrap_or_else(|_| "sma-cross".to_string());
        let realtime = RealtimeLoop::new(
            Arc::clone(&broker),
            Arc::clone(&risk),
            demo_repository(&settings).await,
            repository.clone(),
            settings.watchlist.clone(),
            &strategy_key,
            settings.history_window_bars,
            std::time::Duration::from_millis(500),
        )
        .await;
        let label = realtime.strategy_label();
        tasks.push(tokio::spawn(async move {
            client.run().await;
        }));
        tasks.push(tokio::spawn(realtime_run(realtime, quote_rx)));
        println!(
            "realtime quotes enabled: KIS H0STCNT0 x {} symbols (strategy {}, mock fills only); watchdogs: silence={}s tick-stall={}s (regular session)",
            settings.watchlist.len(),
            strategy_key,
            stale_secs,
            tick_stall_secs
            );
        label
    } else if demo_enabled() || settings.quotes_source == lossfunction::config::QuotesSource::Demo {
        let demo = DemoLoop::new(
            Arc::clone(&broker),
            Arc::clone(&risk),
            demo_repository(&settings).await,
            7,
            settings.watchlist.clone(),
        );
        let label = demo.strategy_label();
        tasks.push(tokio::spawn(demo_run(demo)));
        println!("demo loop enabled (synthetic paper market)");
        label
    } else {
        "—".to_string()
    };

    let state: SharedState = Arc::new(AppState {
        trading_mode: settings.trading_mode.to_string(),
        broker: "MockBroker".to_string(),
        database_path: settings.database_path.clone(),
        risk: Arc::clone(&risk),
        repository: repository.clone(),
        started: std::time::Instant::now(),
        backfill: std::sync::Arc::new(std::sync::Mutex::new(
            lossfunction::runtime::backfill::BackfillStatus::Idle,
        )),
        backfill_creds,
        watchlist: settings.watchlist.clone(),
        strategy_label,
        backtest: std::sync::Arc::new(std::sync::Mutex::new(
            lossfunction::runtime::backfill::BackfillStatus::Idle,
        )),
        screen: std::sync::Arc::new(std::sync::Mutex::new(
            lossfunction::runtime::backfill::BackfillStatus::Idle,
        )),
        optimize: std::sync::Arc::new(std::sync::Mutex::new(
            lossfunction::runtime::backfill::BackfillStatus::Idle,
        )),
        candle_source: if simulation {
            source
        } else {
            repository.clone()
        },
        ws: ws_liveness,
    });

    let app = lossfunction::runtime::server::router(Arc::clone(&state));
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port))
        .await
        .expect("bind health port");
    println!(
        "runtime starting: mode={} broker=MockBroker http=0.0.0.0:{port} (/, /healthz, controls)",
        settings.trading_mode
    );
    axum::serve(listener, app).await.expect("http server");
}

fn demo_enabled() -> bool {
    matches!(
        std::env::var("DEMO_LOOP")
            .unwrap_or_default()
            .to_lowercase()
            .as_str(),
        "1" | "true" | "yes"
    )
}

async fn demo_repository(settings: &Settings) -> Repository {
    Repository::open(&settings.database_path)
        .await
        .expect("open demo sqlite database")
}

async fn sim_run(mut sim: SimLoop) {
    loop {
        if let Err(error) = sim.tick().await {
            eprintln!("sim replay tick failed: {error}");
        }
        if sim.replayed() >= sim.total_bars() {
            println!("sim replay finished ({} bars)", sim.total_bars());
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }
}

async fn demo_run(mut demo: DemoLoop) {
    demo.run().await;
}

async fn realtime_run(
    mut realtime: RealtimeLoop,
    quotes: tokio::sync::mpsc::UnboundedReceiver<lossfunction::types::Quote>,
) {
    realtime.run(quotes).await;
}
