use crate::protocol::*;
use crate::tools::ToolRegistry;
use serde_json::json;
use std::sync::Arc;
use tracing::info;

/// Core Model Context Protocol stateless server handler.
pub struct McpServer {
    registry: Arc<ToolRegistry>,
}

impl McpServer {
    /// Create a new server instance with the given tool registry.
    pub fn new(registry: Arc<ToolRegistry>) -> Self {
        Self { registry }
    }

    /// Handles a stateless JSON-RPC 2.0 request and returns a JSON-RPC response.
    pub async fn handle_request(&self, req: JsonRpcRequest) -> JsonRpcResponse {
        info!("Handling MCP request: id={:?}, method={}", req.id, req.method);

        // Standard JSON-RPC validation
        if req.jsonrpc != "2.0" {
            return JsonRpcResponse::error(
                req.id,
                INVALID_REQUEST,
                "Invalid JSON-RPC version. Expected '2.0'".to_string(),
                None,
            );
        }

        // Trace and telemetry extraction from metadata (_meta)
        if let Some(meta) = req.extract_meta() {
            if let Some(client_info) = meta.client_info {
                info!("Request client info: {} v{}", client_info.name, client_info.version);
            }
            if let Some(traceparent) = meta.traceparent {
                info!("Trace context active: traceparent={}", traceparent);
            }
        }

        match req.method.as_str() {
            "server/discover" => self.handle_discover(req).await,
            "tools/list" => self.handle_tools_list(req).await,
            "tools/call" => self.handle_tools_call(req).await,
            "resources/list" => self.handle_resources_list(req).await,
            "prompts/list" => self.handle_prompts_list(req).await,
            _ => JsonRpcResponse::error(
                req.id,
                METHOD_NOT_FOUND,
                format!("Method not found: {}", req.method),
                None,
            ),
        }
    }

    async fn handle_discover(&self, req: JsonRpcRequest) -> JsonRpcResponse {
        let result = ServerDiscoverResult {
            protocol_version: "2026-07-28".to_string(),
            capabilities: ServerCapabilities {
                tools: Some(json!({})),
                resources: Some(json!({})),
                prompts: Some(json!({})),
            },
            server_info: ServerInfo {
                name: "paramcp".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
        };

        match serde_json::to_value(result) {
            Ok(val) => JsonRpcResponse::success(req.id, val),
            Err(e) => JsonRpcResponse::error(req.id, INTERNAL_ERROR, e.to_string(), None),
        }
    }

    async fn handle_tools_list(&self, req: JsonRpcRequest) -> JsonRpcResponse {
        let tools = self.registry.list_definitions();
        let result = ToolsListResult {
            tools,
            ttl_ms: Some(300_000), // Cache for 5 minutes (stateless optimization)
            cache_scope: Some("shared".to_string()),
        };

        match serde_json::to_value(result) {
            Ok(val) => JsonRpcResponse::success(req.id, val),
            Err(e) => JsonRpcResponse::error(req.id, INTERNAL_ERROR, e.to_string(), None),
        }
    }

    async fn handle_tools_call(&self, req: JsonRpcRequest) -> JsonRpcResponse {
        let params = match req.params.as_ref() {
            Some(p) => p,
            None => {
                return JsonRpcResponse::error(
                    req.id,
                    INVALID_PARAMS,
                    "Missing parameters".to_string(),
                    None,
                );
            }
        };

        let call_params: CallToolParams = match serde_json::from_value(params.clone()) {
            Ok(p) => p,
            Err(e) => {
                return JsonRpcResponse::error(
                    req.id,
                    INVALID_PARAMS,
                    format!("Invalid parameter structure: {}", e),
                    None,
                );
            }
        };

        let tool = match self.registry.get(&call_params.name) {
            Some(t) => t,
            None => {
                return JsonRpcResponse::error(
                    req.id,
                    METHOD_NOT_FOUND,
                    format!("Tool not found: {}", call_params.name),
                    None,
                );
            }
        };

        match tool.call(call_params.arguments).await {
            Ok(res) => match serde_json::to_value(res) {
                Ok(val) => JsonRpcResponse::success(req.id, val),
                Err(e) => JsonRpcResponse::error(req.id, INTERNAL_ERROR, e.to_string(), None),
            },
            Err(e) => {
                // Return tool error in successful JSON-RPC structure with isError=true
                let error_res = ToolCallResult {
                    content: vec![ToolCallContent::Text(ToolCallTextContent {
                        text: format!("Tool execution failed: {}", e),
                    })],
                    is_error: true,
                };
                match serde_json::to_value(error_res) {
                    Ok(val) => JsonRpcResponse::success(req.id, val),
                    Err(err) => JsonRpcResponse::error(req.id, INTERNAL_ERROR, err.to_string(), None),
                }
            }
        }
    }

    async fn handle_resources_list(&self, req: JsonRpcRequest) -> JsonRpcResponse {
        let result = ResourcesListResult {
            resources: vec![],
            ttl_ms: Some(300_000),
            cache_scope: Some("shared".to_string()),
        };
        match serde_json::to_value(result) {
            Ok(val) => JsonRpcResponse::success(req.id, val),
            Err(e) => JsonRpcResponse::error(req.id, INTERNAL_ERROR, e.to_string(), None),
        }
    }

    async fn handle_prompts_list(&self, req: JsonRpcRequest) -> JsonRpcResponse {
        let result = PromptsListResult { prompts: vec![] };
        match serde_json::to_value(result) {
            Ok(val) => JsonRpcResponse::success(req.id, val),
            Err(e) => JsonRpcResponse::error(req.id, INTERNAL_ERROR, e.to_string(), None),
        }
    }
}
