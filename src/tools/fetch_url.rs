use crate::protocol::{ToolCallContent, ToolCallResult, ToolCallTextContent, ToolDefinition};
use crate::tools::Tool;
use regex::Regex;
use std::sync::OnceLock;
use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;

pub struct FetchUrlTool;

impl FetchUrlTool {
    pub fn new() -> Self {
        Self
    }
}

impl Tool for FetchUrlTool {
    fn name(&self) -> &str {
        "fetch_url"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "fetch_url".to_string(),
            description: Some("Fetch web URL (http/https only) and convert to Markdown. SSRF protection: blocks localhost/private/cloud-metadata IPs.".to_string()),
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
                    return Ok(ToolCallResult {
                        content: vec![ToolCallContent::Text(ToolCallTextContent {
                            text: "Error: Missing required argument 'url'".to_string(),
                        })],
                        is_error: true,
                    });
                }
            };

            if let Err(msg) = is_safe_url(url_str) {
                return Ok(error_result(msg));
            }

            // Create client with timeout and user agent
            let client = match reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .user_agent("ParaMCP/0.1.0 Rust High-Performance Agent")
                .build()
            {
                Ok(c) => c,
                Err(e) => return Ok(error_result(format!("Failed to build HTTP client: {}", e))),
            };

            let response = match client.get(url_str).send().await {
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

            // Offload CPU-heavy regex HTML cleaning to blocking pool to keep async runtime responsive
            let markdown = match tokio::task::spawn_blocking(move || clean_html_to_markdown(&html)).await {
                Ok(m) => m,
                Err(e) => return Ok(error_result(format!("Background HTML processing failed: {}", e))),
            };

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
            text: format!("Error: {}", msg),
        })],
        is_error: true,
    }
}

/// Basic SSRF mitigation: only allow public http/https, reject localhost/private IP ranges (string heuristic).
fn is_safe_url(url: &str) -> Result<(), String> {
    let lower = url.to_ascii_lowercase();
    if !lower.starts_with("http://") && !lower.starts_with("https://") {
        return Err("Only http:// or https:// URLs are allowed".to_string());
    }
    // Reject obvious internal targets (best-effort; real protection needs DNS+IP allowlist)
    if lower.contains("localhost") || lower.contains("127.0.0.1") || lower.contains("[::1]") || lower.contains("0.0.0.0") {
        return Err("SSRF blocked: localhost / loopback addresses are not allowed".to_string());
    }
    if lower.contains("10.") || lower.contains("192.168.") || lower.contains("172.16.") || lower.contains("172.17.") || lower.contains("172.18.") || lower.contains("172.31.") {
        return Err("SSRF blocked: private network ranges (10/8, 172.16/12, 192.168/16) are not allowed".to_string());
    }
    if lower.contains("169.254.") || lower.contains("metadata.google") || lower.contains("169.254.169.254") {
        return Err("SSRF blocked: link-local / cloud metadata endpoints are not allowed".to_string());
    }
    Ok(())
}

// Lazily compiled once (performance: avoid 15 regex compiles + unwrap per fetch_url call)
static RE_SCRIPT: OnceLock<Regex> = OnceLock::new();
static RE_STYLE: OnceLock<Regex> = OnceLock::new();
static RE_HEAD: OnceLock<Regex> = OnceLock::new();
static RE_H1: OnceLock<Regex> = OnceLock::new();
static RE_H2: OnceLock<Regex> = OnceLock::new();
static RE_H3: OnceLock<Regex> = OnceLock::new();
static RE_P: OnceLock<Regex> = OnceLock::new();
static RE_BR: OnceLock<Regex> = OnceLock::new();
static RE_LI: OnceLock<Regex> = OnceLock::new();
static RE_A: OnceLock<Regex> = OnceLock::new();
static RE_TAGS: OnceLock<Regex> = OnceLock::new();
static RE_WHITESPACE: OnceLock<Regex> = OnceLock::new();
static RE_NEWLINES: OnceLock<Regex> = OnceLock::new();

/// Convert HTML to basic Markdown using robust, high-performance regex replacements (regexes cached globally).
fn clean_html_to_markdown(html: &str) -> String {
    let mut text = html.to_string();

    // 1. Remove script, style, and head tags entirely (cached)
    let re_script = RE_SCRIPT.get_or_init(|| Regex::new(r"(?is)<script[^>]*>.*?</script>").unwrap());
    text = re_script.replace_all(&text, "").to_string();

    let re_style = RE_STYLE.get_or_init(|| Regex::new(r"(?is)<style[^>]*>.*?</style>").unwrap());
    text = re_style.replace_all(&text, "").to_string();

    let re_head = RE_HEAD.get_or_init(|| Regex::new(r"(?is)<head[^>]*>.*?</head>").unwrap());
    text = re_head.replace_all(&text, "").to_string();

    // 2. Convert headers (cached)
    let re_h1 = RE_H1.get_or_init(|| Regex::new(r"(?i)<h1[^>]*>(.*?)</h1>").unwrap());
    text = re_h1.replace_all(&text, "\n# $1\n").to_string();

    let re_h2 = RE_H2.get_or_init(|| Regex::new(r"(?i)<h2[^>]*>(.*?)</h2>").unwrap());
    text = re_h2.replace_all(&text, "\n## $1\n").to_string();

    let re_h3 = RE_H3.get_or_init(|| Regex::new(r"(?i)<h3[^>]*>(.*?)</h3>").unwrap());
    text = re_h3.replace_all(&text, "\n### $1\n").to_string();

    // 3. Convert paragraphs & line breaks (cached)
    let re_p = RE_P.get_or_init(|| Regex::new(r"(?i)<p[^>]*>(.*?)</p>").unwrap());
    text = re_p.replace_all(&text, "\n$1\n").to_string();

    let re_br = RE_BR.get_or_init(|| Regex::new(r"(?i)<br\s*/?>").unwrap());
    text = re_br.replace_all(&text, "\n").to_string();

    // 4. Convert lists (cached)
    let re_li = RE_LI.get_or_init(|| Regex::new(r"(?i)<li[^>]*>(.*?)</li>").unwrap());
    text = re_li.replace_all(&text, "\n- $1").to_string();

    // 5. Convert links (cached)
    let re_a = RE_A.get_or_init(|| Regex::new(r#"(?i)<a[^>]*href=["']([^"']+)["'][^>]*>(.*?)</a>"#).unwrap());
    text = re_a.replace_all(&text, " [$2]($1) ").to_string();

    // 6. Strip all other remaining HTML tags (cached)
    let re_tags = RE_TAGS.get_or_init(|| Regex::new(r"<[^>]*>").unwrap());
    text = re_tags.replace_all(&text, "").to_string();

    // 7. Decode common HTML entities
    text = text.replace("&nbsp;", " ")
               .replace("&lt;", "<")
               .replace("&gt;", ">")
               .replace("&amp;", "&")
               .replace("&quot;", "\"")
               .replace("&#39;", "'");

    // 8. Normalize spacing and consecutive blank lines (cached)
    let re_whitespace = RE_WHITESPACE.get_or_init(|| Regex::new(r"[ \t]+").unwrap());
    text = re_whitespace.replace_all(&text, " ").to_string();

    let re_newlines = RE_NEWLINES.get_or_init(|| Regex::new(r"\n\s*\n\s*\n+").unwrap());
    text = re_newlines.replace_all(&text, "\n\n").to_string();

    text.trim().to_string()
}
