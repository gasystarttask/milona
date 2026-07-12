//! Phase 4 — Presenter binary entrypoint. Thin: all logic lives in the
//! `milona_presenter` library so both the axum server and the CLI reuse the
//! exact same handlers (see `state::AppState::answer_question`).

use clap::Parser;
use milona_presenter::app::build_router;
use milona_presenter::cli::{build_serve_state, run_query_command, Cli, Command};
use milona_presenter::otel;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // See `otel` module docs: identical output to the previous plain
    // `tracing_subscriber::fmt()` setup unless built with `--features otel`.
    otel::init_tracing();

    let cli = Cli::parse();

    match cli.command {
        Command::Serve { addr } => {
            let (state, dev_key) = build_serve_state().await?;
            if let Some(dev_key) = dev_key {
                tracing::warn!(
                    api_key = %dev_key,
                    "MILONA_API_KEYS not set — generated a single development API key. \
                     Set MILONA_API_KEYS in production."
                );
            }

            let router = build_router(state);
            let listener = tokio::net::TcpListener::bind(&addr).await?;
            tracing::info!(%addr, "milona presenter listening");
            axum::serve(listener, router).await?;
        }
        Command::Query {
            question,
            tenant,
            subject,
        } => {
            let answer = run_query_command(&question, tenant, &subject).await?;
            println!("{answer}");
        }
    }

    Ok(())
}
