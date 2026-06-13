//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Headless Economind API server — thin wrapper around [`economind_api::serve`].

use economind_api::serve;
use economind_config::EconomindConfig;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "economind_api=debug,economind_ingest=info,economind_strategy=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let cfg = EconomindConfig::load()?;
    serve(cfg, None).await
}
