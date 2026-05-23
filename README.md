# ParaMCP

ParaMCP 是一款基于 Rust 开发的、高性能、完全无状态的模型上下文协议 (Model Context Protocol, MCP) 服务端实现。项目严格遵循 **2026-07-28 无状态 MCP 规范草案**，支持本地 STDIO 通信以及基于 HTTP (Axum) 远程通信。

---

## 🎯 产品定位

在大模型 Agent 快速发展的今天，如何让 LLM 安全、高效、低延迟地调用本地或云端的外部工具是关键挑战。
**ParaMCP** 致力于成为 **高性能、云原生友好的大模型工具与上下文接入层**。它通过完全无状态的设计，能够直接部署在云端负载均衡后面进行水平扩展，同时也能够作为本地插件常驻于客户端（如 Cursor、VSCode、Claude Desktop）中。

---

## ⚡ 核心特点

* **完全无状态 (Stateless Core)**
  * 移除传统 MCP 的 `initialize` / `initialized` 握手。
  * 每一个请求都是自包含的，并在请求的 `_meta` 字段中携带版本与客户端信息。
  * 天然支持云端 Round-robin 与水平扩展。
* **双通道传输支持 (Dual Transport)**
  * **STDIO 传输**：适用于本地集成，通过标准输入输出（Stdio）管道流与 IDE 客户端进程进行高速、低延迟的 IPC 通信。
  * **HTTP 传输**：基于 Axum 框架构建，严格校验并路由 2026-07-28 协议要求的 HTTP Headers (`MCP-Protocol-Version`, `Mcp-Method`, `Mcp-Name`)，便于云端远程调用。
* **极致性能 (High Performance)**
  * 基于 Rust 强静态类型和无 GC 运行时，常驻内存仅需数 MB。
  * 异步 I/O 基于 Tokio 驱动，保障微秒级分发响应。
* **内置的高性能工具集**
  * `sys_info`: 实时硬件指标监控（CPU 负载、内存分配、OS 及主机信息）。
  * `calculator`: 手写递归下降解析器，高效计算数学算式。
  * `file_search`: 高效的目录递归 Regex 正则内容检索，智能避开二进制与大文件。
  * `fetch_url`: 抓取网页并自动利用 Regex 规则转化为干净的 Markdown 格式文本。

---

## 🛠 架构设计

```
 ┌───────────────────────┐
 │   MCP Client / Host   │
 └───────────┬───────────┘
             │ (STDIO / HTTP POST)
             ▼
 ┌───────────────────────┐
 │   Transport Router    │
 └───────────┬───────────┘
             │ (JSON-RPC 2.0)
             ▼
 ┌───────────────────────┐
 │   MCP Server Engine   │
 └───────────┬───────────┘
             ├───────────────────┬──────────────┐
             ▼                   ▼              ▼
     ┌───────────────┐   ┌───────────────┐  ┌───────────────┐
     │  sys_info     │   │ calculator    │  │ file_search   │ ...
     └───────────────┘   └───────────────┘  └───────────────┘
```

---

## 🚀 快速开始

### 1. 编译构建

确保您已安装 Rust 稳定版工具链。

```bash
cargo build --release
```

编译产物位于 `target/release/paramcp`。

### 2. 命令行参数

```bash
# 启动 STDIO 模式 (本地客户端对接)
./target/release/paramcp --transport stdio

# 启动 HTTP 模式 (开放远程端口 8080)
./target/release/paramcp --transport http --port 8080
```

---

## 📖 使用示例

### STDIO 模式验证

通过管道向 Stdio 模式写入一个标准 JSON-RPC 发现包：

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"server/discover","params":{}}' | ./target/release/paramcp --transport stdio
```

### HTTP 模式验证

发送 HTTP 请求时，**必须**携带 MCP 路由标头：

```bash
curl -X POST http://127.0.0.1:8080/mcp \
  -H "MCP-Protocol-Version: 2026-07-28" \
  -H "Mcp-Method: server/discover" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"server/discover","params":{}}'
```

调用计算器工具 (`calculator`) 示例：

```bash
curl -X POST http://127.0.0.1:8080/mcp \
  -H "MCP-Protocol-Version: 2026-07-28" \
  -H "Mcp-Method: tools/call" \
  -H "Mcp-Name: calculator" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"calculator","arguments":{"expr":"2 + 3 * (10 - 4)"}}}'
```

---

## 🧪 自动化测试

运行单元测试和真实的 Socket HTTP 传输集成测试：

```bash
cargo test
```

## 📄 许可证

本项目基于 MIT 许可证开源。
