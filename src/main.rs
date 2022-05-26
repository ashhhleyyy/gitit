use axum::Extension;
use clap::Parser;
use config::Config;
use tower_http::trace::TraceLayer;
use tracing_subscriber::prelude::*;

mod config;
mod errors;
mod routes;
mod update;
mod utils;

#[derive(clap::Parser)]
enum Cli {
    Web,
    UpdateRepos,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::filter::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "debug,hyper=info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let cli = Cli::parse();
    let config = config::load()?;
    match cli {
        Cli::Web => run_server(config).await,
        Cli::UpdateRepos => update::update_repos(config).map_err(|e| e.into()),
    }
}

async fn run_server(config: Config) -> Result<(), Box<dyn std::error::Error>> {
    let app = routes::build_router()
        .layer(TraceLayer::new_for_http())
        .layer(Extension(config.clone()));

    axum::Server::bind(&config.server.address.parse().unwrap())
        .serve(app.into_make_service())
        .await?;
    
    Ok(())
}
