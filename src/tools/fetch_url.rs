use crate::protocol::{ToolCallContent, ToolCallResult, ToolCallTextContent, ToolDefinition};
use crate::tools::Tool;
use regex::Regex;
use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;

pub struct FetchUrlTool {
    client: reqwest::Client,
    re_script: Regex,
    re_style: Regex,
    re_head: Regex,
    re_h1: Regex,
    re_h2: Regex,
    re_h3: Regex,
    re_p: Regex,
    re_br: Regex,
    re_li: Regex,
    re_a: Regex,
    re_tags: Regex,
    re_whitespace: Regex,
    re_newlines: Regex,
}

impl FetchUrlTool {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .user_agent("ParaMCP/0.1.0 Rust High-Performance Agent")
            .build()
            .expect("Failed to build reqwest Client");

        Self {
            client,
            re_script: Regex::new(r"(?is)<script[^>]*>.*?</script>").unwrap(),
            re_style: Regex::new(r"(?is)<style[^>]*>.*?</style>").unwrap(),
            re_head: Regex::new(r"(?is)<head[^>]*>.*?</head>").unwrap(),
            re_h1: Regex::new(r"(?i)<h1[^>]*>(.*?)</h1>").unwrap(),
            re_h2: Regex::new(r"(?i)<h2[^>]*>(.*?)</h2>").unwrap(),
            re_h3: Regex::new(r"(?i)<h3[^>]*>(.*?)</h3>").unwrap(),
            re_p: Regex::new(r"(?i)<p[^>]*>(.*?)</p>").unwrap(),
            re_br: Regex::new(r"(?i)<br\s*/?>").unwrap(),
            re_li: Regex::new(r"(?i)<li[^>]*>(.*?)</li>").unwrap(),
            re_a: Regex::new(r#"(?i)<a[^>]*href=["']([^"']+)["'][^>]*>(.*?)</a>"#).unwrap(),
            re_tags: Regex::new(r"<[^>]*>").unwrap(),
            re_whitespace: Regex::new(r"[ \t]+").unwrap(),
            re_newlines: Regex::new(r"\n\s*\n\s*\n+").unwrap(),
        }
    }

    fn clean_html_to_markdown(&self, html: &str) -> String {
        let mut text = html.to_string();

        // 1. Remove script, style, and head tags entirely
        text = self.re_script.replace_all(&text, "").to_string();
        text = self.re_style.replace_all(&text, "").to_string();
        text = self.re_head.replace_all(&text, "").to_string();

        // 2. Convert headers
        text = self.re_h1.replace_all(&text, "\n# $1\n").to_string();
        text = self.re_h2.replace_all(&text, "\n## $1\n").to_string();
        text = self.re_h3.replace_all(&text, "\n### $1\n").to_string();

        // 3. Convert paragraphs & line breaks
        text = self.re_p.replace_all(&text, "\n$1\n").to_string();
        text = self.re_br.replace_all(&text, "\n").to_string();

        // 4. Convert lists
        text = self.re_li.replace_all(&text, "\n- $1").to_string();

        // 5. Convert links: <a href="url">text</a> -> [text](url)
        text = self.re_a.replace_all(&text, " [$2]($1) ").to_string();

        // 6. Strip all other remaining HTML tags
        text = self.re_tags.replace_all(&text, "").to_string();

        // 7. Decode common HTML entities
        text = text.replace("&nbsp;", " ")
                   .replace("&lt;", "<")
                   .replace("&gt;", ">")
                   .replace("&amp;", "&")
                   .replace("&quot;", "\"")
                   .replace("&#39;", "'");

        // 8. Normalize spacing and consecutive blank lines
        text = self.re_whitespace.replace_all(&text, " ").to_string();
        text = self.re_newlines.replace_all(&text, "\n\n").to_string();

        text.trim().to_string()
    }
}

impl Default for FetchUrlTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for FetchUrlTool {
    fn name(&self) -> &str {
        "fetch_url"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "fetch_url".to_string(),
            description: Some("Fetch content from a web URL and convert it to clean readable Markdown text.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The URL to fetch, e.g. 'https://news.ycombinator.com'"
                    }
                },
                "required": ["url"]
            }),
        }
    }

    fn call(&self, arguments: Option<Value>) -> Pin<Box<dyn Future<Output = anyhow::Result<ToolCallResult>> + Send + '_>> {
        Box::pin(async move {
            let url_str = match arguments.as_ref().and_then(|a| a.get("url").and_then(|u| u.as_str())) {
                Some(u) => u,
                None => {
                    return Ok(error_result("Error: Missing required argument 'url'".to_string()));
                }
            };

            let response = match self.client.get(url_str).send().await {
                Ok(r) => r,
                Err(e) => return Ok(error_result(format!("HTTP Request failed: {}", e))),
            };

            let status = response.status();
            if !status.is_success() {
                return Ok(error_result(format!("Server returned error status: {}", status)));
            }

            let html = match response.text().await {
                Ok(t) => t,
                Err(e) => return Ok(error_result(format!("Failed to read response text: {}", e))),
            };

            let markdown = self.clean_html_to_markdown(&html);

            Ok(ToolCallResult {
                content: vec![ToolCallContent::Text(ToolCallTextContent {
                    text: markdown,
                })],
                is_error: false,
            })
        })
    }
}

fn error_result(msg: String) -> ToolCallResult {
    ToolCallResult {
        content: vec![ToolCallContent::Text(ToolCallTextContent {
            text: msg,
        })],
        is_error: true,
    }
}
