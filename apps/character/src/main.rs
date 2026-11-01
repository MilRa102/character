//! The `character` application.
//!
//! Today it runs the headless core daemon; the drawer UI joins in MVP-0.

use anyhow::Result;
use character_core::Core;
use character_types::Mode;
use clap::Parser;
use tokio::signal::{
    self,
    unix::{SignalKind, signal},
};
use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;

/// AI companion for the COSMIC desktop that lives in a drawer at the top of the screen.
#[derive(Debug, Parser)]
#[command(version, about)]
struct Cli {}

#[tokio::main]
async fn main() -> Result<()> {
    Cli::parse();

    let filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy();
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let mut sigterm = signal(SignalKind::terminate())?;
    let shutdown = async move {
        tokio::select! {
            _ = signal::ctrl_c() => {}
            _ = sigterm.recv() => {}
        }
    };

    Core::new(Mode::default()).run(shutdown).await;
    Ok(())
}
