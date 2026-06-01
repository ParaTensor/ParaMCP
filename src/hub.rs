use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::process::{Child, Command};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot, Mutex};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{error, info, warn};
use anyhow::Result;
use crate::protocol::{JsonRpcRequest, JsonRpcResponse, RequestId, ToolDefinition, INTERNAL_ERROR};
use std::sync::Mutex as StdMutex;

type PendingRequestMap = HashMap<u64, (Option<RequestId>, oneshot::Sender<JsonRpcResponse>)>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubServerConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub protocol_version: String, // "2026-07-28" or "legacy"
    pub env: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubConfig {
    pub sub_servers: Vec<SubServerConfig>,
}

pub struct SubprocessHost {
    name: String,
    protocol_version: String,
    tx_request: mpsc::Sender<(JsonRpcRequest, oneshot::Sender<JsonRpcResponse>)>,
    child: Arc<Mutex<Child>>,
}

impl SubprocessHost {
    pub async fn new(config: SubServerConfig) -> Result<Self> {
        let name = config.name.clone();
        let protocol_version = config.protocol_version.clone();
        
        let (tx_request, mut rx_request) = mpsc::channel::<(JsonRpcRequest, oneshot::Sender<JsonRpcResponse>)>(100);
        
        // Spawn the child process
        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args)
           .stdin(Stdio::piped())
           .stdout(Stdio::piped())
           .stderr(Stdio::inherit()); // Inherit stderr so child logs print to our console

        if let Some(ref envs) = config.env {
            cmd.envs(envs);
        }

        let mut child = cmd.spawn()?;
        let mut stdin = child.stdin.take().ok_or_else(|| anyhow::anyhow!("Failed to open stdin of child"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow::anyhow!("Failed to open stdout of child"))?;
        
        let pending_requests: Arc<Mutex<PendingRequestMap>> =
            Arc::new(Mutex::new(HashMap::new()));
            
        let next_id = Arc::new(AtomicU64::new(1));
        
        // Background thread to read stdout from the child
        let pending_clone = Arc::clone(&pending_requests);
        let name_clone = name.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                
                // Parse response
                if let Ok(mut resp) = serde_json::from_str::<JsonRpcResponse>(trimmed) {
                    if let Some(ref id) = resp.id {
                        let id_u64 = match id {
                            RequestId::Number(n) => *n as u64,
                            RequestId::String(s) => s.parse::<u64>().unwrap_or(0),
                        };
                        
                        let mut pending = pending_clone.lock().await;
                        if let Some((orig_id, tx)) = pending.remove(&id_u64) {
                            resp.id = orig_id;
                            let _ = tx.send(resp);
                        } else {
                            warn!("Received response for untracked ID: {:?}", id);
                        }
                    }
                } else {
                    error!("Child {} sent invalid JSON-RPC: {}", name_clone, trimmed);
                }
            }
            info!("Child {} read loop terminated", name_clone);
            // Drain all pending requests and notify them of the subprocess termination
            let mut pending = pending_clone.lock().await;
            for (_internal_id, (orig_id, tx)) in pending.drain() {
                let _ = tx.send(JsonRpcResponse::error(
                    orig_id,
                    INTERNAL_ERROR,
                    format!("Subprocess '{}' terminated unexpectedly", name_clone),
                    None,
                ));
            }
        });

        // Background thread to write to stdin of the child
        let pending_clone_w = Arc::clone(&pending_requests);
        let next_id_clone = Arc::clone(&next_id);
        let name_clone_w = name.clone();
        tokio::spawn(async move {
            while let Some((mut req, tx)) = rx_request.recv().await {
                if req.id.is_none() {
                    // It's a notification, send it and resolve the oneshot immediately
                    if let Ok(line) = serde_json::to_string(&req) {
                        let _ = stdin.write_all(line.as_bytes()).await;
                        let _ = stdin.write_all(b"\n").await;
                        let _ = stdin.flush().await;
                    }
                    let _ = tx.send(JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id: None,
                        result: None,
                        error: None,
                    });
                    continue;
                }
                
                let internal_id = next_id_clone.fetch_add(1, Ordering::SeqCst);
                let orig_id = req.id.clone();
                req.id = Some(RequestId::Number(internal_id as i64));
                
                {
                    let mut pending = pending_clone_w.lock().await;
                    pending.insert(internal_id, (orig_id, tx));
                }
                
                if let Ok(line) = serde_json::to_string(&req) {
                    if let Err(e) = stdin.write_all(line.as_bytes()).await {
                        error!("Failed to write to child {}: {}", name_clone_w, e);
                    }
                    let _ = stdin.write_all(b"\n").await;
                    let _ = stdin.flush().await;
                }
            }
        });

        let host = Self {
            name,
            protocol_version,
            tx_request,
            child: Arc::new(Mutex::new(child)),
        };

        // If the server is legacy, run initialize handshake
        if host.protocol_version == "legacy" {
            host.initialize_legacy().await?;
        }

        Ok(host)
    }

    async fn initialize_legacy(&self) -> Result<()> {
        info!("Initializing legacy stateful server {}...", self.name);
        
        let init_req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(RequestId::Number(0)),
            method: "initialize".to_string(),
            params: Some(json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "paramcp-hub",
                    "version": "0.1.0"
                }
            })),
        };
        
        let (tx, rx) = oneshot::channel();
        self.tx_request.send((init_req, tx)).await?;
        let resp = rx.await?;
        if let Some(err) = resp.error {
            return Err(anyhow::anyhow!("Legacy init failed: {:?}", err));
        }
        
        // Send initialized notification (no response expected per JSON-RPC spec)
        let initialized_notification = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: None,
            method: "notifications/initialized".to_string(),
            params: None,
        };
        
        let (tx_n, _rx_n) = oneshot::channel();
        self.tx_request.send((initialized_notification, tx_n)).await?;
        // Notifications do not expect responses; the writer loop resolves the channel immediately
        
        info!("Legacy server {} initialized successfully.", self.name);
        Ok(())
    }

    pub async fn call(&self, req: JsonRpcRequest) -> Result<JsonRpcResponse> {
        let (tx, rx) = oneshot::channel();
        self.tx_request.send((req, tx)).await?;
        Ok(rx.await?)
    }

    /// Gracefully kill the underlying child process.
    pub async fn shutdown(&self) -> Result<()> {
        info!("Shutting down subprocess '{}'...", self.name);
        let mut child = self.child.lock().await;
        if let Err(e) = child.kill().await {
            warn!("Failed to send kill signal to '{}': {}", self.name, e);
        }
        Ok(())
    }

    /// Check if the child process is still alive.
    pub async fn is_alive(&self) -> bool {
        match self.child.lock().await.try_wait() {
            Ok(None) => true,
            Ok(Some(status)) => {
                warn!("Subprocess '{}' has exited with status: {}", self.name, status);
                false
            }
            Err(e) => {
                warn!("Failed to check status of subprocess '{}': {}", self.name, e);
                false
            }
        }
    }
}

pub struct HubManager {
    // Option allows us to take() in Drop for async cleanup
    hosts: Option<StdMutex<HashMap<String, Arc<SubprocessHost>>>>,
    // exposed_tool_name -> (subserver_name, original_tool_name)
    tool_routing: StdMutex<HashMap<String, (String, String)>>,
    // Merged tool definitions to return in tools/list
    merged_tools: StdMutex<Vec<ToolDefinition>>,
}

impl HubManager {
    pub async fn new(config_path: &Path) -> Result<Arc<Self>> {
        let mut hosts = HashMap::new();
        let mut tool_routing = HashMap::new();
        let mut merged_tools = Vec::new();

        // Read and parse config
        let config_content = std::fs::read_to_string(config_path)?;
        let config: HubConfig = serde_json::from_str(&config_content)?;

        // Spawn child hosts
        for sub_cfg in config.sub_servers {
            info!("Spawning subprocess MCP host '{}'...", sub_cfg.name);
            let host = Arc::new(SubprocessHost::new(sub_cfg.clone()).await?);
            hosts.insert(sub_cfg.name.clone(), Arc::clone(&host));

            // Fetch tools list from this child
            let list_req = JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                id: Some(RequestId::Number(1)),
                method: "tools/list".to_string(),
                params: None,
            };
            
            match host.call(list_req).await {
                Ok(resp) => {
                    info!("Successfully fetched tools from child '{}'", sub_cfg.name);
                    if let Some(err) = resp.error {
                        error!("Failed to fetch tools from child '{}': {:?}", sub_cfg.name, err);
                    } else if let Some(result) = resp.result {
                        if let Some(tools_val) = result.get("tools").and_then(|t| t.as_array()) {
                            for tool_val in tools_val {
                                if let Ok(mut tool_def) = serde_json::from_value::<ToolDefinition>(tool_val.clone()) {
                                    let original_name = tool_def.name.clone();
                                    
                                    // Verify namespace collision with built-ins or existing tools
                                    let is_conflict = tool_routing.contains_key(&original_name) 
                                        || original_name == "sys_info"
                                        || original_name == "calculator"
                                        || original_name == "file_search"
                                        || original_name == "fetch_url";
                                        
                                    let exposed_name = if is_conflict {
                                        format!("{}__{}", sub_cfg.name, original_name)
                                    } else {
                                        original_name.clone()
                                    };
                                    
                                    tool_def.name = exposed_name.clone();
                                    merged_tools.push(tool_def);
                                    
                                    tool_routing.insert(
                                        exposed_name,
                                        (sub_cfg.name.clone(), original_name)
                                    );
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to communicate with child '{}' during startup: {}", sub_cfg.name, e);
                }
            }
        }

        let manager = Arc::new(Self {
            hosts: Some(StdMutex::new(hosts)),
            tool_routing: StdMutex::new(tool_routing),
            merged_tools: StdMutex::new(merged_tools),
        });

        Self::spawn_health_monitor(&manager);
        Ok(manager)
    }

    pub fn empty() -> Arc<Self> {
        Arc::new(Self {
            hosts: Some(StdMutex::new(HashMap::new())),
            tool_routing: StdMutex::new(HashMap::new()),
            merged_tools: StdMutex::new(Vec::new()),
        })
    }

    pub fn get_merged_tools(&self) -> Vec<ToolDefinition> {
        self.merged_tools.lock().unwrap().clone()
    }

    pub fn get_routing(&self, tool_name: &str) -> Option<(String, String)> {
        self.tool_routing.lock().unwrap().get(tool_name).cloned()
    }

    pub fn get_host(&self, name: &str) -> Option<Arc<SubprocessHost>> {
        self.hosts.as_ref()?.lock().unwrap().get(name).cloned()
    }

    /// Gracefully shutdown all managed subprocess hosts.
    pub async fn shutdown(&self) {
        if let Some(ref hosts_lock) = self.hosts {
            let hosts: Vec<(String, Arc<SubprocessHost>)> = {
                let hosts = hosts_lock.lock().unwrap();
                hosts.iter().map(|(k, v)| (k.clone(), Arc::clone(v))).collect()
            };
            for (name, host) in hosts {
                info!("Stopping host '{}'...", name);
                if let Err(e) = host.shutdown().await {
                    error!("Failed to shutdown host '{}': {}", name, e);
                }
            }
        }
    }

    /// Spawn a background task that periodically checks subprocess health.
    pub fn spawn_health_monitor(self: &Arc<Self>) {
        let manager = Arc::clone(self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
            loop {
                interval.tick().await;
                manager.check_health().await;
            }
        });
    }

    async fn check_health(&self) {
        // Snapshot hosts without holding the lock across await points
        let hosts_snapshot: Vec<(String, Arc<SubprocessHost>)> = match &self.hosts {
            Some(lock) => {
                let hosts = lock.lock().unwrap();
                hosts.iter().map(|(k, v)| (k.clone(), Arc::clone(v))).collect()
            }
            None => return,
        };

        let mut dead_hosts = Vec::new();
        for (name, host) in hosts_snapshot {
            if !host.is_alive().await {
                warn!("Health check: subprocess host '{}' is dead", name);
                dead_hosts.push(name);
            }
        }

        for name in dead_hosts {
            // Remove from hosts and attempt shutdown
            let host_to_shutdown = if let Some(lock) = &self.hosts {
                let mut hosts = lock.lock().unwrap();
                hosts.remove(&name)
            } else {
                None
            };
            if let Some(host) = host_to_shutdown {
                if let Err(e) = host.shutdown().await {
                    error!("Error shutting down dead host '{}': {}", name, e);
                }
            }

            // Identify tools belonging to this host
            let tools_to_remove: Vec<String> = {
                let routing = self.tool_routing.lock().unwrap();
                routing.iter()
                    .filter(|(_, (srv, _))| srv == &name)
                    .map(|(tool, _)| tool.clone())
                    .collect()
            };

            // Remove from routing
            {
                let mut routing = self.tool_routing.lock().unwrap();
                for tool in &tools_to_remove {
                    routing.remove(tool);
                }
            }

            // Remove from merged tools
            {
                let mut merged = self.merged_tools.lock().unwrap();
                merged.retain(|t| !tools_to_remove.contains(&t.name));
            }

            if !tools_to_remove.is_empty() {
                info!("Cleaned up {} tools from dead host '{}'", tools_to_remove.len(), name);
            }
        }
    }
}

impl Drop for HubManager {
    fn drop(&mut self) {
        if let Some(hosts_lock) = self.hosts.take() {
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    if let Ok(hosts) = hosts_lock.into_inner() {
                        for (name, host) in hosts {
                            if let Err(e) = host.shutdown().await {
                                tracing::error!("Failed to shutdown host '{}': {}", name, e);
                            }
                        }
                    }
                });
            }
        }
    }
}
