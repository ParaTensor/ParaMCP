use paramcp::protocol::{JsonRpcRequest, RequestId};
use paramcp::server::McpServer;
use paramcp::tools::ToolRegistry;
use serde_json::json;
use std::sync::Arc;
use paramcp::hub::HubManager;

#[tokio::test]
async fn test_server_discover() {
    let registry = Arc::new(ToolRegistry::new());
    let server = McpServer::new(registry, Arc::new(HubManager::empty()));

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
    let server = McpServer::new(registry, Arc::new(HubManager::empty()));

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
    let server = McpServer::new(registry, Arc::new(HubManager::empty()));

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
    let server = McpServer::new(registry, Arc::new(HubManager::empty()));

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
    let server = McpServer::new(registry, Arc::new(HubManager::empty()));

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set in test");
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
    let server = McpServer::new(registry, Arc::new(HubManager::empty()));
    
    // Find an unused local port
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let server_handle = tokio::spawn(async move {
        paramcp::transport::http::run_http_transport(Arc::new(server), "127.0.0.1", port).await.unwrap();
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
    let server = McpServer::new(registry, Arc::new(HubManager::empty()));

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
    
    // Dynamically build portable test config using CARGO_MANIFEST_DIR + scratch mocks
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let scratch = format!("{}/scratch", manifest);
    let stateless_py = format!("{}/stateless_mock.py", scratch);
    let legacy_py = format!("{}/legacy_mock.py", scratch);
    let temp_config_path = format!("/tmp/paramcp_test_config_{}.json", std::process::id());
    let config_json = format!(
        r#"{{
  "sub_servers": [
    {{
      "name": "stateless-mock",
      "command": "python3",
      "args": ["{}"],
      "protocol_version": "2026-07-28"
    }},
    {{
      "name": "legacy-mock",
      "command": "python3",
      "args": ["{}"],
      "protocol_version": "legacy"
    }}
  ]
}}"#,
        stateless_py, legacy_py
    );
    std::fs::write(&temp_config_path, &config_json).expect("Failed to write temp test config");
    let config_path = std::path::Path::new(&temp_config_path);
    let hub = Arc::new(HubManager::new(config_path).await.expect("Failed to initialize HubManager with portable config"));
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
