use std::sync::Arc;

use saturn::{
    app::{AppConfig, AppState, build_router},
    nostr::SdkNostrPublisher,
    payments::{MockLightningAdapter, MockOnChainAdapter},
    persistence::{
        PostgresNonceRepository, PostgresOrderRepository, PostgresQuoteRepository,
        PostgresReceiptRepository, connect,
    },
};
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let config = AppConfig::from_env()?;
    init_tracing(&config);

    let pool = connect(&config.database_url).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;

    let state = AppState::new(
        config.clone(),
        Arc::new(PostgresQuoteRepository::new(pool.clone())),
        Arc::new(PostgresOrderRepository::new(pool.clone())),
        Arc::new(PostgresReceiptRepository::new(pool.clone())),
        Arc::new(PostgresNonceRepository::new(pool)),
        Arc::new(MockLightningAdapter),
        Arc::new(MockOnChainAdapter),
        Arc::new(SdkNostrPublisher::new(&config)?),
    );
    let app = build_router(state);
    let listener = TcpListener::bind(&config.server_addr).await?;
    info!(addr = %config.server_addr, "starting saturn server");
    axum::serve(listener, app).await?;
    Ok(())
}

fn init_tracing(config: &AppConfig) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let subscriber = fmt().with_env_filter(filter);
    if config.log_format == "json" {
        subscriber.json().init();
    } else {
        subscriber.pretty().init();
    }
}
