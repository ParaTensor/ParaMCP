use crate::protocol::*;
use crate::tools::ToolRegistry;
use serde_json::json;
use std::sync::Arc;
use tracing::info;

/// Core Model Context Protocol stateless server handler.
pub struct McpServer {
    registry: Arc<ToolRegistry>,
    hub: Arc<crate::hub::HubManager>,
}

impl McpServer {
    /// Create a new server instance with the given tool registry and hub manager.
    pub fn new(registry: Arc<ToolRegistry>, hub: Arc<crate::hub::HubManager>) -> Self {
        Self { registry, hub }
    }

    /// Handles a stateless or stateful JSON-RPC 2.0 request and returns a JSON-RPC response.
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
            "initialize" => self.handle_initialize(req).await,
            "notifications/initialized" => self.handle_initialized_notification(req).await,
            "server/discover" => self.handle_discover(req).await,
            "tools/list" => self.handle_tools_list(req).await,
            "tools/call" => self.handle_tools_call(req).await,
            "resources/list" => self.handle_resources_list(req).await,
            "resources/read" => self.handle_resources_read(req).await,
            "prompts/list" => self.handle_prompts_list(req).await,
            "prompts/get" => self.handle_prompts_get(req).await,
            _ => JsonRpcResponse::error(
                req.id,
                METHOD_NOT_FOUND,
                format!("Method not found: {}", req.method),
                None,
            ),
        }
    }

    async fn handle_initialize(&self, req: JsonRpcRequest) -> JsonRpcResponse {
        // Return initialize capabilities compatible with legacy clients
        let client_version = req.params.as_ref()
            .and_then(|p| p.get("protocolVersion"))
            .and_then(|v| v.as_str())
            .unwrap_or("2024-11-05");

        let result = json!({
            "protocolVersion": client_version,
            "capabilities": {
                "tools": {},
                "resources": {},
                "prompts": {}
            },
            "serverInfo": {
                "name": "paramcp-hub",
                "version": env!("CARGO_PKG_VERSION")
            }
        });

        JsonRpcResponse::success(req.id, result)
    }

    async fn handle_initialized_notification(&self, req: JsonRpcRequest) -> JsonRpcResponse {
        // Notifications do not expect standard JSON-RPC success response values, but return success/empty
        JsonRpcResponse::success(req.id, json!({}))
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
        let mut tools = self.registry.list_definitions();
        let sub_tools = self.hub.get_merged_tools();
        tools.extend(sub_tools.iter().cloned());

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

        // Check if the tool belongs to a proxy subserver
        if let Some((subserver_name, original_tool_name)) = self.hub.get_routing(&call_params.name) {
            if let Some(host) = self.hub.get_host(&subserver_name) {
                // Rewrite the request parameter 'name' to the original name expected by the child
                let mut modified_params = params.clone();
                if let Some(obj) = modified_params.as_object_mut() {
                    obj.insert("name".to_string(), json!(original_tool_name));
                }
                
                let mut modified_req = req.clone();
                modified_req.params = Some(modified_params);
                
                match host.call(modified_req).await {
                    Ok(resp) => return resp,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            req.id,
                            INTERNAL_ERROR,
                            format!("Failed to proxy call to subserver '{}': {}", subserver_name, e),
                            None,
                        );
                    }
                }
            }
        }

        // If not routed to a subserver, execute locally
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
            resources: vec![
                ResourceDefinition {
                    uri: "paramcp://server/info".to_string(),
                    name: "Server Info".to_string(),
                    description: Some("Basic metadata about the ParaMCP server instance.".to_string()),
                    mime_type: Some("application/json".to_string()),
                },
            ],
            ttl_ms: Some(300_000),
            cache_scope: Some("shared".to_string()),
        };
        match serde_json::to_value(result) {
            Ok(val) => JsonRpcResponse::success(req.id, val),
            Err(e) => JsonRpcResponse::error(req.id, INTERNAL_ERROR, e.to_string(), None),
        }
    }

    async fn handle_resources_read(&self, req: JsonRpcRequest) -> JsonRpcResponse {
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

        let read_params: ReadResourceParams = match serde_json::from_value(params.clone()) {
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

        match read_params.uri.as_str() {
            "paramcp://server/info" => {
                let text = json!({
                    "name": "paramcp",
                    "version": env!("CARGO_PKG_VERSION"),
                    "protocolVersion": "2026-07-28",
                });
                let result = ResourceReadResult {
                    contents: vec![ResourceContent {
                        uri: read_params.uri,
                        mime_type: Some("application/json".to_string()),
                        text: Some(text.to_string()),
                        blob: None,
                    }],
                };
                match serde_json::to_value(result) {
                    Ok(val) => JsonRpcResponse::success(req.id, val),
                    Err(e) => JsonRpcResponse::error(req.id, INTERNAL_ERROR, e.to_string(), None),
                }
            }
            _ => JsonRpcResponse::error(
                req.id,
                METHOD_NOT_FOUND,
                format!("Resource not found: {}", read_params.uri),
                None,
            ),
        }
    }

    async fn handle_prompts_list(&self, req: JsonRpcRequest) -> JsonRpcResponse {
        let result = PromptsListResult {
            prompts: vec![
                PromptDefinition {
                    name: "explain-code".to_string(),
                    description: Some("Explain a piece of code in plain language.".to_string()),
                    arguments: Some(vec![
                        PromptArgument {
                            name: "language".to_string(),
                            description: Some("Programming language of the code.".to_string()),
                            required: Some(true),
                        },
                        PromptArgument {
                            name: "code".to_string(),
                            description: Some("The code snippet to explain.".to_string()),
                            required: Some(true),
                        },
                    ]),
                },
            ],
        };
        match serde_json::to_value(result) {
            Ok(val) => JsonRpcResponse::success(req.id, val),
            Err(e) => JsonRpcResponse::error(req.id, INTERNAL_ERROR, e.to_string(), None),
        }
    }

    async fn handle_prompts_get(&self, req: JsonRpcRequest) -> JsonRpcResponse {
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

        let get_params: GetPromptParams = match serde_json::from_value(params.clone()) {
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

        match get_params.name.as_str() {
            "explain-code" => {
                let language = get_params
                    .arguments
                    .as_ref()
                    .and_then(|a| a.get("language"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let code = get_params
                    .arguments
                    .as_ref()
                    .and_then(|a| a.get("code"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                let result = PromptGetResult {
                    description: Some(format!("Explain the following {} code.", language)),
                    messages: vec![
                        PromptMessage {
                            role: "user".to_string(),
                            content: PromptContent::Text(PromptMessageText {
                                text: format!(
                                    "Please explain this {} code in plain language:\n\n```{}\n{}\n```",
                                    language, language, code
                                ),
                            }),
                        },
                    ],
                };
                match serde_json::to_value(result) {
                    Ok(val) => JsonRpcResponse::success(req.id, val),
                    Err(e) => JsonRpcResponse::error(req.id, INTERNAL_ERROR, e.to_string(), None),
                }
            }
            _ => JsonRpcResponse::error(
                req.id,
                METHOD_NOT_FOUND,
                format!("Prompt not found: {}", get_params.name),
                None,
            ),
        }
    }
}
