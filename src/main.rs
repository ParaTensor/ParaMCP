use clap::{Parser, ValueEnum};
use paramcp::server::McpServer;
use paramcp::tools::ToolRegistry;
use paramcp::transport::{http::run_http_transport, stdio::run_stdio_transport};
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::EnvFilter;

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

    /// Host to bind on (HTTP only). Defaults to 127.0.0.1 for security (exposes no dangerous tools to network).
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Path to the Hub configuration JSON file
    #[arg(short, long)]
    config: Option<std::path::PathBuf>,
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
        Arc::new(paramcp::hub::HubManager::new(path).await?)
    } else {
        Arc::new(paramcp::hub::HubManager::empty())
    };

    let registry = Arc::new(ToolRegistry::new());
    let server = Arc::new(McpServer::new(registry, hub));

    match args.transport {
        TransportType::Stdio => {
            run_stdio_transport(server).await?;
        }
        TransportType::Http => {
            run_http_transport(server, &args.host, args.port).await?;
        }
    }

    Ok(())
}
