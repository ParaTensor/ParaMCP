use crate::protocol::{ToolCallContent, ToolCallResult, ToolCallTextContent, ToolDefinition};
use crate::tools::Tool;
use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;
use sysinfo::System;

pub struct SysInfoTool;

impl SysInfoTool {
    pub fn new() -> Self {
        Self
    }
}

impl Tool for SysInfoTool {
    fn name(&self) -> &str {
        "sys_info"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "sys_info".to_string(),
            description: Some("Fetch high-performance system metrics (CPU load, memory usage, host details).".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        }
    }

    fn call(&self, _arguments: Option<Value>) -> Pin<Box<dyn Future<Output = anyhow::Result<ToolCallResult>> + Send + '_>> {
        Box::pin(async move {
            let mut sys = System::new_all();
            sys.refresh_all();

            let total_mem = sys.total_memory();
            let used_mem = sys.used_memory();
            let mem_usage = if total_mem > 0 {
                (used_mem as f64 / total_mem as f64) * 100.0
            } else {
                0.0
            };

            let cpu_count = sys.cpus().len();
            let cpu_usage = sys.global_cpu_info().cpu_usage();

            let os_name = System::name().unwrap_or_else(|| "Unknown".to_string());
            let os_version = System::os_version().unwrap_or_else(|| "Unknown".to_string());
            let kernel_version = System::kernel_version().unwrap_or_else(|| "Unknown".to_string());
            let hostname = System::host_name().unwrap_or_else(|| "Unknown".to_string());

            let res = json!({
                "hostname": hostname,
                "os": format!("{} v{}", os_name, os_version),
                "kernel": kernel_version,
                "cpu": {
                    "cores": cpu_count,
                    "usage_percent": format!("{:.2}%", cpu_usage)
                },
                "memory": {
                    "total_bytes": total_mem,
                    "used_bytes": used_mem,
                    "usage_percent": format!("{:.2}%", mem_usage)
                }
            });

            Ok(ToolCallResult {
                content: vec![ToolCallContent::Text(ToolCallTextContent {
                    text: serde_json::to_string_pretty(&res)?,
                })],
                is_error: false,
            })
        })
    }
}
