use paramcp::protocol::{JsonRpcRequest, RequestId};
use paramcp::server::McpServer;
use paramcp::tools::ToolRegistry;
use serde_json::json;
use std::sync::Arc;
use paramcp::hub::HubManager;

#[tokio::test]
async fn test_server_discover() {
    let registry = Arc::new(ToolRegistry::new());
    let server = McpServer::new(registry, HubManager::empty());

    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(RequestId::Number(1)),
        method: "server/discover".to_string(),
        params: None,
    };

    let resp = server.handle_request(req).await;
    assert_eq!(resp.jsonrpc, "2.0");
    assert_eq!(resp.id, Some(RequestId::Number(1)));
    assert!(resp.error.is_none());

    let result = resp.result.unwrap();
    assert_eq!(result.get("protocolVersion").unwrap().as_str().unwrap(), "2026-07-28");
    assert_eq!(result.get("serverInfo").unwrap().get("name").unwrap().as_str().unwrap(), "paramcp");
}

#[tokio::test]
async fn test_tools_list() {
    let registry = Arc::new(ToolRegistry::new());
    let server = McpServer::new(registry, HubManager::empty());

    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(RequestId::Number(2)),
        method: "tools/list".to_string(),
        params: None,
    };

    let resp = server.handle_request(req).await;
    assert!(resp.error.is_none());

    let result = resp.result.unwrap();
    let tools = result.get("tools").unwrap().as_array().unwrap();
    assert!(tools.len() >= 4); // sys_info, calculator, file_search, fetch_url

    let tool_names: Vec<&str> = tools.iter().map(|t| t.get("name").unwrap().as_str().unwrap()).collect();
    assert!(tool_names.contains(&"sys_info"));
    assert!(tool_names.contains(&"calculator"));
    assert!(tool_names.contains(&"file_search"));
    assert!(tool_names.contains(&"fetch_url"));

    assert_eq!(result.get("ttlMs").unwrap().as_u64().unwrap(), 300_000);
    assert_eq!(result.get("cacheScope").unwrap().as_str().unwrap(), "shared");
}

#[tokio::test]
async fn test_tools_call_calculator() {
    let registry = Arc::new(ToolRegistry::new());
    let server = McpServer::new(registry, HubManager::empty());

    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(RequestId::String("calc".to_string())),
        method: "tools/call".to_string(),
        params: Some(json!({
            "name": "calculator",
            "arguments": {
                "expr": "10 + 5 * 2"
            }
        })),
    };

    let resp = server.handle_request(req).await;
    assert!(resp.error.is_none());

    let result = resp.result.unwrap();
    assert!(!result.get("isError").unwrap().as_bool().unwrap());
    
    let content = result.get("content").unwrap().as_array().unwrap();
    let text = content[0].get("text").unwrap().as_str().unwrap();
    assert_eq!(text, "20");
}

#[tokio::test]
async fn test_tools_call_sys_info() {
    let registry = Arc::new(ToolRegistry::new());
    let server = McpServer::new(registry, HubManager::empty());

    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(RequestId::Number(3)),
        method: "tools/call".to_string(),
        params: Some(json!({
            "name": "sys_info",
            "arguments": {}
        })),
    };

    let resp = server.handle_request(req).await;
    assert!(resp.error.is_none());

    let result = resp.result.unwrap();
    assert!(!result.get("isError").unwrap().as_bool().unwrap());
    
    let content = result.get("content").unwrap().as_array().unwrap();
    let text = content[0].get("text").unwrap().as_str().unwrap();
    assert!(text.contains("hostname"));
    assert!(text.contains("cpu"));
    assert!(text.contains("memory"));
}

#[tokio::test]
async fn test_tools_call_file_search() {
    let registry = Arc::new(ToolRegistry::new());
    let server = McpServer::new(registry, HubManager::empty());

    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(RequestId::Number(4)),
        method: "tools/call".to_string(),
        params: Some(json!({
            "name": "file_search",
            "arguments": {
                "dir": manifest_dir,
                "pattern": "paramcp",
                "extension": "toml"
            }
        })),
    };

    let resp = server.handle_request(req).await;
    assert!(resp.error.is_none());

    let result = resp.result.unwrap();
    assert!(!result.get("isError").unwrap().as_bool().unwrap());
    
    let content = result.get("content").unwrap().as_array().unwrap();
    let text = content[0].get("text").unwrap().as_str().unwrap();
    assert!(text.contains("Cargo.toml"));
    assert!(text.contains("paramcp"));
}

#[tokio::test]
async fn test_http_transport() {
    let registry = Arc::new(ToolRegistry::new());
    let server = McpServer::new(registry, HubManager::empty());
    
    // Find an unused local port
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let server_handle = tokio::spawn(async move {
        paramcp::transport::http::run_http_transport(Arc::new(server), port, None, None).await.unwrap();
    });

    // Wait a brief moment for the server to start
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/mcp", port);

    // 1. Valid request
    let resp = client.post(&url)
        .header("MCP-Protocol-Version", "2026-07-28")
        .header("Mcp-Method", "server/discover")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 10,
            "method": "server/discover",
            "params": {}
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body.get("jsonrpc").unwrap().as_str().unwrap(), "2.0");
    assert_eq!(body.get("id").unwrap().as_i64().unwrap(), 10);
    assert_eq!(body.get("result").unwrap().get("protocolVersion").unwrap().as_str().unwrap(), "2026-07-28");

    // 2. Request missing MCP-Protocol-Version header
    let resp_missing_ver = client.post(&url)
        .header("Mcp-Method", "server/discover")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 11,
            "method": "server/discover",
            "params": {}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp_missing_ver.status(), reqwest::StatusCode::BAD_REQUEST);

    // 3. Request with mismatched Mcp-Method header
    let resp_mismatched_method = client.post(&url)
        .header("MCP-Protocol-Version", "2026-07-28")
        .header("Mcp-Method", "tools/list")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 12,
            "method": "server/discover",
            "params": {}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp_mismatched_method.status(), reqwest::StatusCode::BAD_REQUEST);

    // Stop server
    server_handle.abort();
}

#[tokio::test]
async fn test_invalid_request() {
    let registry = Arc::new(ToolRegistry::new());
    let server = McpServer::new(registry, HubManager::empty());

    // Invalid jsonrpc version
    let req = JsonRpcRequest {
        jsonrpc: "1.0".to_string(),
        id: Some(RequestId::Number(100)),
        method: "server/discover".to_string(),
        params: None,
    };

    let resp = server.handle_request(req).await;
    assert!(resp.error.is_some());
    assert_eq!(resp.error.unwrap().code, -32600); // Invalid request
}

#[tokio::test]
async fn test_hub_aggregation_and_proxy() {
    let _ = tracing_subscriber::fmt().try_init();
    let registry = Arc::new(ToolRegistry::new());
    
    // Spawn HubManager using our test config
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let config_content = std::fs::read_to_string(format!("{}/scratch/test_config.json", manifest_dir))
        .expect("Failed to read test config");
    let config_content = config_content.replace("/home/xinference/github/ParaMCP", manifest_dir);
    let temp_path = std::env::temp_dir().join("paramcp_test_config.json");
    std::fs::write(&temp_path, config_content).expect("Failed to write temp config");
    
    let hub = HubManager::new(&temp_path).await.expect("Failed to initialize HubManager");
    let server = McpServer::new(registry, Arc::clone(&hub));

    // 1. Check tools list aggregation
    let list_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(RequestId::Number(1)),
        method: "tools/list".to_string(),
        params: None,
    };
    let resp = server.handle_request(list_req).await;
    assert!(resp.error.is_none());
    
    let result = resp.result.unwrap();
    let tools = result.get("tools").unwrap().as_array().unwrap();
    
    let tool_names: Vec<&str> = tools.iter().map(|t| t.get("name").unwrap().as_str().unwrap()).collect();
    // Built-in tools
    assert!(tool_names.contains(&"sys_info"));
    assert!(tool_names.contains(&"calculator"));
    // Subprocess tools
    assert!(tool_names.contains(&"stateless_tool"));
    assert!(tool_names.contains(&"legacy_tool"));

    // 2. Test proxying tools/call to stateless-mock subserver
    let call_stateless = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(RequestId::Number(2)),
        method: "tools/call".to_string(),
        params: Some(json!({
            "name": "stateless_tool",
            "arguments": {}
        })),
    };
    let resp_stateless = server.handle_request(call_stateless).await;
    assert!(resp_stateless.error.is_none());
    let res_stateless = resp_stateless.result.unwrap();
    let content_stateless = res_stateless.get("content").unwrap().as_array().unwrap();
    let text_stateless = content_stateless[0].get("text").unwrap().as_str().unwrap();
    assert_eq!(text_stateless, "Hello from Stateless Tool");

    // 3. Test proxying tools/call to legacy-mock subserver (requires initialize handshake)
    let call_legacy = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(RequestId::Number(3)),
        method: "tools/call".to_string(),
        params: Some(json!({
            "name": "legacy_tool",
            "arguments": {}
        })),
    };
    let resp_legacy = server.handle_request(call_legacy).await;
    assert!(resp_legacy.error.is_none());
    let res_legacy = resp_legacy.result.unwrap();
    let content_legacy = res_legacy.get("content").unwrap().as_array().unwrap();
    let text_legacy = content_legacy[0].get("text").unwrap().as_str().unwrap();
    assert_eq!(text_legacy, "Hello from Legacy Tool");
}

#[tokio::test]
async fn test_resources_read() {
    let registry = Arc::new(ToolRegistry::new());
    let server = McpServer::new(registry, HubManager::empty());

    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(RequestId::Number(5)),
        method: "resources/read".to_string(),
        params: Some(json!({"uri": "paramcp://server/info"})),
    };

    let resp = server.handle_request(req).await;
    assert!(resp.error.is_none());

    let result = resp.result.unwrap();
    let contents = result.get("contents").unwrap().as_array().unwrap();
    assert_eq!(contents.len(), 1);
    assert_eq!(contents[0].get("uri").unwrap().as_str().unwrap(), "paramcp://server/info");
    let text = contents[0].get("text").unwrap().as_str().unwrap();
    assert!(text.contains("paramcp"));
}

#[tokio::test]
async fn test_prompts_get() {
    let registry = Arc::new(ToolRegistry::new());
    let server = McpServer::new(registry, HubManager::empty());

    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(RequestId::Number(6)),
        method: "prompts/get".to_string(),
        params: Some(json!({
            "name": "explain-code",
            "arguments": {
                "language": "rust",
                "code": "fn main() {}"
            }
        })),
    };

    let resp = server.handle_request(req).await;
    assert!(resp.error.is_none());

    let result = resp.result.unwrap();
    let messages = result.get("messages").unwrap().as_array().unwrap();
    assert_eq!(messages.len(), 1);
    let text = messages[0].get("content").unwrap().get("text").unwrap().as_str().unwrap();
    assert!(text.contains("rust"));
    assert!(text.contains("fn main() {}"));
}

#[tokio::test]
async fn test_health_endpoint() {
    let registry = Arc::new(ToolRegistry::new());
    let server = McpServer::new(registry, HubManager::empty());

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let server_handle = tokio::spawn(async move {
        paramcp::transport::http::run_http_transport(Arc::new(server), port, None, None).await.unwrap();
    });

    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://127.0.0.1:{}/health", port))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body.get("status").unwrap().as_str().unwrap(), "ok");
    assert_eq!(body.get("version").unwrap().as_str().unwrap(), env!("CARGO_PKG_VERSION"));

    server_handle.abort();
}

#[tokio::test]
async fn test_http_api_key_auth() {
    let registry = Arc::new(ToolRegistry::new());
    let server = McpServer::new(registry, HubManager::empty());

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let api_key = "secret-key-123".to_string();
    let server_handle = tokio::spawn(async move {
        paramcp::transport::http::run_http_transport(
            Arc::new(server),
            port,
            Some(api_key),
            None,
        )
        .await
        .unwrap();
    });

    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/mcp", port);

    // 1. Missing API key
    let resp_no_key = client
        .post(&url)
        .header("MCP-Protocol-Version", "2026-07-28")
        .header("Mcp-Method", "server/discover")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "server/discover",
            "params": {}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp_no_key.status(), reqwest::StatusCode::UNAUTHORIZED);

    // 2. Wrong API key
    let resp_wrong = client
        .post(&url)
        .header("MCP-Protocol-Version", "2026-07-28")
        .header("Mcp-Method", "server/discover")
        .header("X-Api-Key", "wrong-key")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "server/discover",
            "params": {}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp_wrong.status(), reqwest::StatusCode::UNAUTHORIZED);

    // 3. Correct API key
    let resp_ok = client
        .post(&url)
        .header("MCP-Protocol-Version", "2026-07-28")
        .header("Mcp-Method", "server/discover")
        .header("X-Api-Key", "secret-key-123")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "server/discover",
            "params": {}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp_ok.status(), reqwest::StatusCode::OK);

    server_handle.abort();
}
