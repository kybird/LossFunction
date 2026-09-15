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
use lossfunction::storage::Repository;

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

    let state: SharedState = Arc::new(AppState {
        trading_mode: settings.trading_mode.to_string(),
        broker: "MockBroker".to_string(),
        database_path: settings.database_path.clone(),
        risk: Arc::clone(&risk),
        repository,
        started: std::time::Instant::now(),
    });

    let mut tasks = Vec::new();
    if demo_enabled() {
        let demo = DemoLoop::new(
            Arc::clone(&broker),
            Arc::clone(&risk),
            demo_repository(&settings).await,
            7,
        );
        tasks.push(tokio::spawn(demo_run(demo)));
        println!("demo loop enabled (synthetic paper market)");
    }

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

async fn demo_run(mut demo: DemoLoop) {
    demo.run().await;
}
