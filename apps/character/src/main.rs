//! The `character` application.
//!
//! Opens the MVP-0 drawer by default, or the core lifecycle with `--headless`.

use anyhow::{Context, Result};
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
struct Cli {
    /// Run only the core daemon, without a display or drawer.
    #[arg(long)]
    headless: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy();
    tracing_subscriber::fmt().with_env_filter(filter).init();

    if cli.headless {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?
            .block_on(run_headless())
    } else {
        anyhow::ensure!(
            std::env::var_os("WAYLAND_DISPLAY").is_some_and(|value| !value.is_empty())
                || std::env::var_os("WAYLAND_SOCKET").is_some_and(|value| !value.is_empty()),
            "a Wayland session with layer-shell support is required; use --headless for the core only"
        );
        character_ui::run().context("could not start the drawer; a Wayland session with layer-shell support is required (use --headless for the core only)")
    }
}

async fn run_headless() -> Result<()> {
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
