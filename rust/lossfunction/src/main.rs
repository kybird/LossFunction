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
use lossfunction::runtime::server::{AppState, SharedState};
use lossfunction::runtime::sim::SimLoop;
use lossfunction::storage::Repository;

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

    let to = chrono::Utc::now();
    let from = to - chrono::Duration::days(365 * years);
    println!(
        "backfill: {} symbols x {}y -> {} (domain: {})",
        settings.watchlist.len(),
        years,
        settings.database_path,
        environment
    );
    let outcomes = backfill_daily(&source, &repository, &settings.watchlist, from, to).await;
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

    let risk = Arc::new(RiskManager::new(RiskLimits::from(settings.risk)));
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

    let mut tasks = Vec::new();
    let strategy_label = if simulation {
        let source_path =
            std::env::var("SIM_SOURCE").unwrap_or_else(|_| settings.database_path.clone());
        let source = Repository::open(&source_path)
            .await
            .expect("open simulation source database");
        source.migrate().await.expect("migrate simulation source");
        let sim = SimLoop::new(
            Arc::clone(&broker),
            Arc::clone(&risk),
            demo_repository(&settings).await,
            source,
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
    } else if demo_enabled() {
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
        repository,
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
