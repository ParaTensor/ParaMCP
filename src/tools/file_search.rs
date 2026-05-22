use crate::protocol::{ToolCallContent, ToolCallResult, ToolCallTextContent, ToolDefinition};
use crate::tools::Tool;
use regex::Regex;
use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

pub struct FileSearchTool;

impl FileSearchTool {
    pub fn new() -> Self {
        Self
    }
}

impl Tool for FileSearchTool {
    fn name(&self) -> &str {
        "file_search"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "file_search".to_string(),
            description: Some("Search for exact regex patterns inside text files in a directory recursively.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "dir": {
                        "type": "string",
                        "description": "Absolute directory path to search, e.g. '/home/xinference/github/ParaMCP'"
                    },
                    "pattern": {
                        "type": "string",
                        "description": "Regex pattern to look for inside file contents"
                    },
                    "extension": {
                        "type": "string",
                        "description": "Optional file extension filter, e.g. 'rs' or 'toml'"
                    }
                },
                "required": ["dir", "pattern"]
            }),
        }
    }

    fn call(&self, arguments: Option<Value>) -> Pin<Box<dyn Future<Output = anyhow::Result<ToolCallResult>> + Send + '_>> {
        Box::pin(async move {
            let dir_str = match arguments.as_ref().and_then(|a| a.get("dir").and_then(|d| d.as_str())) {
                Some(d) => d,
                None => return missing_arg("dir"),
            };
            let pattern_str = match arguments.as_ref().and_then(|a| a.get("pattern").and_then(|p| p.as_str())) {
                Some(p) => p,
                None => return missing_arg("pattern"),
            };
            let extension = arguments.as_ref().and_then(|a| a.get("extension").and_then(|e| e.as_str()));

            let dir_path = Path::new(dir_str);
            if !dir_path.exists() || !dir_path.is_dir() {
                return Ok(error_result(format!("Directory does not exist or is not a directory: {}", dir_str)));
            }

            let regex = match Regex::new(pattern_str) {
                Ok(r) => r,
                Err(e) => return Ok(error_result(format!("Invalid regex pattern: {}", e))),
            };

            let mut matches = Vec::new();
            let max_matches = 100;

            if let Err(e) = search_dir(dir_path, &regex, extension, &mut matches, max_matches) {
                return Ok(error_result(format!("Error searching files: {}", e)));
            }

            let output = json!({
                "dir": dir_str,
                "pattern": pattern_str,
                "matches_count": matches.len(),
                "matches": matches
            });

            Ok(ToolCallResult {
                content: vec![ToolCallContent::Text(ToolCallTextContent {
                    text: serde_json::to_string_pretty(&output)?,
                })],
                is_error: false,
            })
        })
    }
}

fn missing_arg(name: &str) -> anyhow::Result<ToolCallResult> {
    Ok(ToolCallResult {
        content: vec![ToolCallContent::Text(ToolCallTextContent {
            text: format!("Error: Missing required argument '{}'", name),
        })],
        is_error: true,
    })
}

fn error_result(msg: String) -> ToolCallResult {
    ToolCallResult {
        content: vec![ToolCallContent::Text(ToolCallTextContent {
            text: format!("Error: {}", msg),
        })],
        is_error: true,
    }
}

fn is_binary_file(path: &Path) -> std::io::Result<bool> {
    let mut file = File::open(path)?;
    let mut buffer = [0; 1024];
    let bytes_read = file.read(&mut buffer)?;
    
    // Check for null bytes in the first 1024 bytes
    for &byte in &buffer[..bytes_read] {
        if byte == 0 {
            return Ok(true);
        }
    }
    Ok(false)
}

fn search_dir(
    dir: &Path,
    regex: &Regex,
    extension: Option<&str>,
    matches: &mut Vec<Value>,
    max_matches: usize,
) -> std::io::Result<()> {
    if matches.len() >= max_matches {
        return Ok(());
    }

    let entries = std::fs::read_dir(dir)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            // Skip hidden directories (like .git, .target)
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with('.') || name == "target" {
                    continue;
                }
            }
            search_dir(&path, regex, extension, matches, max_matches)?;
        } else if path.is_file() {
            // Apply extension filter
            if let Some(ext) = extension {
                if path.extension().and_then(|e| e.to_str()) != Some(ext) {
                    continue;
                }
            }

            // Skip binary files
            if let Ok(true) = is_binary_file(&path) {
                continue;
            }

            search_file(&path, regex, matches, max_matches)?;
        }
    }
    Ok(())
}

fn search_file(
    path: &Path,
    regex: &Regex,
    matches: &mut Vec<Value>,
    max_matches: usize,
) -> std::io::Result<()> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    for (line_num, line) in reader.lines().enumerate() {
        if matches.len() >= max_matches {
            break;
        }

        let line = match line {
            Ok(l) => l,
            Err(_) => continue, // Skip lines that are not valid UTF-8
        };

        if regex.is_match(&line) {
            matches.push(json!({
                "file": path.to_string_lossy().to_string(),
                "line": line_num + 1,
                "content": line.trim()
            }));
        }
    }
    Ok(())
}
