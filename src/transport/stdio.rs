use crate::protocol::{JsonRpcRequest, JsonRpcResponse, PARSE_ERROR};
use crate::server::McpServer;
use std::sync::Arc;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{error, info};

/// Runs the STDIO-based transport loop, parsing JSON-RPC line-by-line.
pub async fn run_stdio_transport(server: Arc<McpServer>) -> io::Result<()> {
    info!("Starting STDIO MCP transport loop...");
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut reader = BufReader::new(stdin).lines();

    while let Some(line) = reader.next_line().await? {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Deserialize request
        let req: JsonRpcRequest = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(e) => {
                error!("JSON-RPC parse error on stdin: {}", e);
                let resp = JsonRpcResponse::error(
                    None,
                    PARSE_ERROR,
                    format!("Parse error: {}", e),
                    None,
                );
                if let Ok(resp_str) = serde_json::to_string(&resp) {
                    stdout.write_all(resp_str.as_bytes()).await?;
                    stdout.write_all(b"\n").await?;
                    stdout.flush().await?;
                }
                continue;
            }
        };

        // Process request
        let resp = server.handle_request(req).await;

        // Serialize and output response
        match serde_json::to_string(&resp) {
            Ok(resp_str) => {
                if let Err(e) = stdout.write_all(resp_str.as_bytes()).await {
                    error!("Stdout write error: {}", e);
                    break;
                }
                if let Err(e) = stdout.write_all(b"\n").await {
                    error!("Stdout newline write error: {}", e);
                    break;
                }
                if let Err(e) = stdout.flush().await {
                    error!("Stdout flush error: {}", e);
                    break;
                }
            }
            Err(e) => {
                error!("Serialization error for response: {}", e);
            }
        }
    }

    info!("STDIO transport loop stopped.");
    Ok(())
}
