pub mod calculator;
pub mod fetch_url;
pub mod file_search;
pub mod sys_info;

use crate::protocol::{ToolCallResult, ToolDefinition};
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

/// Trait defining an MCP tool that can be invoked asynchronously.
pub trait Tool: Send + Sync {
    /// Unique identifier of the tool.
    fn name(&self) -> &str;

    /// The MCP definition of the tool (including input schema).
    fn definition(&self) -> ToolDefinition;

    /// Execute the tool with the given arguments.
    fn call(&self, arguments: Option<Value>) -> Pin<Box<dyn Future<Output = anyhow::Result<ToolCallResult>> + Send + '_>>;
}

/// A thread-safe registry containing all available tools.
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    /// Create a new registry and register all default tools.
    pub fn new() -> Self {
        let mut registry = Self {
            tools: HashMap::new(),
        };
        registry.register(sys_info::SysInfoTool::new());
        registry.register(calculator::CalculatorTool::new());
        registry.register(file_search::FileSearchTool::new());
        registry.register(fetch_url::FetchUrlTool::new());
        registry
    }

    /// Register a new tool.
    pub fn register<T: Tool + 'static>(&mut self, tool: T) {
        self.tools.insert(tool.name().to_string(), Box::new(tool));
    }

    /// Retrieve a reference to a registered tool by name.
    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|t| t.as_ref())
    }

    /// Return definition metadata of all registered tools.
    pub fn list_definitions(&self) -> Vec<ToolDefinition> {
        self.tools.values().map(|t| t.definition()).collect()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
