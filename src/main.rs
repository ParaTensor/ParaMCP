use clap::{Parser, ValueEnum};
use paramcp::server::McpServer;
use paramcp::tools::ToolRegistry;
use paramcp::transport::{http::run_http_transport, stdio::run_stdio_transport};
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::EnvFilter;

/// Waits for SIGINT (Ctrl+C) or SIGTERM (Unix only) shutdown signals.
async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();

    #[cfg(unix)]
    {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler");
        tokio::select! {
            _ = ctrl_c => { info!("Received SIGINT (Ctrl+C)"); }
            _ = sigterm.recv() => { info!("Received SIGTERM"); }
        }
    }

    #[cfg(not(unix))]
    {
        let _ = ctrl_c.await;
        info!("Received SIGINT (Ctrl+C)");
    }
}

#[derive(Parser, Debug)]
#[command(name = "paramcp")]
#[command(author = "EeroEternal")]
#[command(version = "0.1.0")]
#[command(about = "High-performance Model Context Protocol (MCP) server", long_about = None)]
struct Args {
    /// The transport layer to run on
    #[arg(short, long, value_enum, default_value_t = TransportType::Stdio)]
    transport: TransportType,

    /// Port to listen on (only applicable for HTTP transport)
    #[arg(short, long, default_value_t = 8080)]
    port: u16,

    /// Path to the Hub configuration JSON file
    #[arg(short, long)]
    config: Option<std::path::PathBuf>,

    /// Optional API key for HTTP transport authentication (X-Api-Key header)
    #[arg(long, env = "PARAMCP_API_KEY")]
    api_key: Option<String>,

    /// CORS allowed origin for HTTP transport. Use '*' to allow any origin.
    #[arg(long, env = "PARAMCP_ALLOW_ORIGIN")]
    allow_origin: Option<String>,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
enum TransportType {
    Stdio,
    Http,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Setup tracing/logging output
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("paramcp=info,tower_http=debug")),
        )
        .init();

    let args = Args::parse();
    info!("Starting ParaMCP v{}...", env!("CARGO_PKG_VERSION"));

    let hub = if let Some(ref path) = args.config {
        paramcp::hub::HubManager::new(path).await?
    } else {
        paramcp::hub::HubManager::empty()
    };

    let registry = Arc::new(ToolRegistry::new());
    let server = Arc::new(McpServer::new(registry, Arc::clone(&hub)));

    match args.transport {
        TransportType::Stdio => {
            tokio::select! {
                result = run_stdio_transport(server) => { result?; }
                _ = shutdown_signal() => { info!("Shutting down STDIO transport gracefully..."); }
            }
        }
        TransportType::Http => {
            tokio::select! {
                result = run_http_transport(server, args.port, args.api_key, args.allow_origin) => { result?; }
                _ = shutdown_signal() => { info!("Shutting down HTTP transport gracefully..."); }
            }
        }
    }

    hub.shutdown().await;
    info!("ParaMCP shutdown complete.");
    Ok(())
}
