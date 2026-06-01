use crate::protocol::JsonRpcRequest;
use crate::server::McpServer;
use axum::{
    extract::{Extension, State},
    http::{HeaderMap, Method, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info};

/// Handler for the POST /mcp endpoint. Performs strict MCP 2026-07-28 header validation.
pub async fn handle_mcp_post(
    State(server): State<Arc<McpServer>>,
    Extension(api_key): Extension<Option<String>>,
    headers: HeaderMap,
    Json(payload): Json<JsonRpcRequest>,
) -> impl IntoResponse {
    // 0. Verify API key if configured
    if let Some(expected) = api_key {
        let provided = headers.get("X-Api-Key").and_then(|v| v.to_str().ok());
        if provided != Some(&expected) {
            return (
                StatusCode::UNAUTHORIZED,
                "Invalid or missing X-Api-Key header",
            )
                .into_response();
        }
    }

    // 1. Verify MCP-Protocol-Version header is present
    let version_header = match headers.get("MCP-Protocol-Version").and_then(|v| v.to_str().ok()) {
        Some(v) => v,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                "Missing or invalid MCP-Protocol-Version header",
            )
                .into_response();
        }
    };
    info!("Client requested protocol version: {}", version_header);

    // 2. Verify Mcp-Method header exists and matches the body method field
    let method_header = match headers.get("Mcp-Method").and_then(|v| v.to_str().ok()) {
        Some(m) => m,
        None => {
            return (StatusCode::BAD_REQUEST, "Missing or invalid Mcp-Method header").into_response();
        }
    };

    if method_header != payload.method {
        error!(
            "Header Mcp-Method '{}' mismatches JSON-RPC method '{}'",
            method_header, payload.method
        );
        return (
            StatusCode::BAD_REQUEST,
            format!(
                "Mcp-Method header '{}' does not match JSON-RPC body method '{}'",
                method_header, payload.method
            ),
        )
            .into_response();
    }

    // 3. For tools/call, verify Mcp-Name header exists and matches parameters name
    if payload.method == "tools/call" {
        let name_header = match headers.get("Mcp-Name").and_then(|v| v.to_str().ok()) {
            Some(n) => n,
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    "Missing or invalid Mcp-Name header for tools/call",
                )
                    .into_response();
            }
        };

        let tool_name = payload
            .params
            .as_ref()
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str());

        match tool_name {
            Some(name) => {
                if name_header != name {
                    error!(
                        "Header Mcp-Name '{}' mismatches parameters tool name '{}'",
                        name_header, name
                    );
                    return (
                        StatusCode::BAD_REQUEST,
                        format!(
                            "Mcp-Name header '{}' does not match parameter name '{}'",
                            name_header, name
                        ),
                    )
                        .into_response();
                }
            }
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    "Missing tool name inside request parameters object",
                )
                    .into_response();
            }
        }
    }

    // 4. Handle request statelessly
    let resp = server.handle_request(payload).await;
    Json(resp).into_response()
}

/// Simple health check endpoint.
pub async fn handle_health() -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

/// Start the high-performance Axum HTTP server listener on the specified port.
pub async fn run_http_transport(
    server: Arc<McpServer>,
    port: u16,
    api_key: Option<String>,
    allow_origin: Option<String>,
) -> anyhow::Result<()> {
    let mut cors = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST])
        .allow_headers(Any);

    if let Some(origin) = allow_origin {
        if origin == "*" {
            cors = cors.allow_origin(Any);
        } else {
            let origin_val = origin.parse().map_err(|e| {
                anyhow::anyhow!("Invalid CORS allow-origin value '{}': {}", origin, e)
            })?;
            cors = cors.allow_origin(tower_http::cors::AllowOrigin::exact(origin_val));
        }
    }

    let app = Router::new()
        .route("/mcp", post(handle_mcp_post))
        .route("/health", get(handle_health))
        .layer(Extension(api_key))
        .layer(cors)
        .with_state(server);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!("Starting high-performance HTTP MCP server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
