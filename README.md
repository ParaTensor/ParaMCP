# ParaMCP

ParaMCP 是一款基于 Rust 开发的、高性能、完全无状态的模型上下文协议 (Model Context Protocol, MCP) 统一管理中心与路由网关。项目支持 **2026-07-28 新版无状态草案**以及 **旧版有状态（Legacy）** 双协议兼容，并集成了内置高效工具与多进程下游子服务生命周期管理功能。

---

## 🎯 产品定位

在大模型 Agent 快速发展的今天，如何安全、高效、低延迟地管理和调用散落在不同环境（如 Python、Node.js 等）中的工具集是核心痛点。
**ParaMCP** 致力于作为 **独立运行的统一工具调度中枢 (Unified MCP Hub)**。它运行在客户端/网关与底层的具体工具实现之间，不仅可以毫秒级执行自身的高性能 Rust 内置工具，更能拉起、托管并代理转发外部的其他任意 MCP 子服务进程，实现 Agent 的无感极简对接。

---

## ⚡ 核心特点

* **统一进程托管与路由代理 (Subprocess Orchestration)**
  * 通过配置文件一键管理多个 Python、Node.js 等第三方 MCP 子进程的生命周期，随主服务自动拉起、监控、容错与销毁。
  * 提供跨进程的工具代理分发（Tools Proxy Routing），并在 Rust 层面利用 tokio 异步 I/O 实现超低延迟的 JSON-RPC 消息管道转发。
  * **工具名命名空间冲突消解**：如果不同子服务间存在同名工具冲突，ParaMCP 会自动增加双下划线命名空间前缀（如 `server_name__tool_name`）并提供无感翻译。
* **双协议与双传输兼容 (Dual-Protocol & Dual-Transport)**
  * **旧版有状态（Legacy）与新版无状态（2026-07-28）无缝兼容**：对上层 Agent 屏蔽协议版本差异。若 Agent 发起 `initialize` 握手，则启用会话协商；若 Agent 直连则直接解包；对于下游子服务，ParaMCP 会在后台自动按需进行 initialization 握手并缓存其能力。
  * **STDIO 与 HTTP 双通道**：支持基于 Stdio IPC 的本地极速通信（对齐 Cursor、Claude Desktop 等）以及基于 Axum 的远程 HTTP API 转发。
* **极致性能 (High Performance)**
  * 常驻内存仅需数 MB，消除冷启动延迟与垃圾回收抖动。
  * 支持 JSON-RPC 请求标识符（ID）的高并发重映射，防止多进程间调用 ID 碰撞。
* **内置的高性能工具集**
  * `sys_info`: 实时硬件指标监控（CPU 负载、内存分配、OS 及主机信息）。
  * `calculator`: 手写递归下降解析器，高效计算数学算式。
  * `file_search`: 高效的目录递归 Regex 正则内容检索，智能避开二进制与大文件。
  * `fetch_url`: 抓取网页并自动利用 Regex 规则转化为干净的 Markdown 格式文本。

---

## 🛠 架构设计

```
                            ┌───────────────────────────┐
                            │    Agent / ParaGateway    │
                            └─────────────┬─────────────┘
                                          │ (Old Stateful OR New Stateless)
                                          ▼
  ┌───────────────────────────────────────────────────────────────────────────────┐
  │ ParaMCP Hub (Rust 统一管理中心)                                                 │
  │                                                                               │
  │  ┌───────────────────────────┐              ┌──────────────────────────────┐  │
  │  │   Multi-Protocol Adapter  │              │    Process Supervisor        │  │
  │  │ - 兼容旧版 initialize 握手 │              │ - 异步管理子进程生命周期      │  │
  │  └─────────────┬─────────────┘              └──────────────┬───────────────┘  │
  │                │                                           │                  │
  │                ▼                                           ▼                  │
  │  ┌─────────────────────────────────────────────────────────────────────────┐  │
  │  │   Tool Aggregator & Router                                              │  │
  │  │   - 本地工具执行 / 外部代理路由                                          │  │
  │  └───────────────────┬───────────────┬──────────────────────┬──────────────┘  │
  └──────────────────────┼───────────────┼──────────────────────┼─────────────────┘
                         │               │                      │
                         ▼ (进程内执行)   ▼ (Stdio 管道)         ▼ (Stdio 管道)
                 ┌──────────────┐┌──────────────┐┌──────────────┐
                 │ Built-in Rust││ Python MCP   ││ Node.js MCP  │
                 │   Tools      ││ (Stateful)   ││ (Stateless)  │
                 └──────────────┘└──────────────┘└──────────────┘
```

---

## ⚙ 配置文件说明 (`paramcp_config.json`)

Hub 启动时可以通过 `--config` 载入托管配置文件，配置多个子 MCP 服务：

```json
{
  "sub_servers": [
    {
      "name": "brave-search",
      "command": "node",
      "args": ["/path/to/brave-search/index.js"],
      "protocol_version": "2026-07-28",
      "env": {
        "BRAVE_API_KEY": "your_key_here"
      }
    },
    {
      "name": "legacy-db-service",
      "command": "python3",
      "args": ["/path/to/db_tool.py"],
      "protocol_version": "legacy"
    }
  ]
}
```

---

## 🚀 快速开始

### 1. 编译构建

```bash
cargo build --release
```

### 2. 命令行参数

```bash
# 启动 STDIO 模式，并载入托管配置
./target/release/paramcp --transport stdio --config ./paramcp_config.json

# 启动 HTTP 模式，并挂载托管服务于 8080 端口
./target/release/paramcp --transport http --port 8080 --config ./paramcp_config.json
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

---

## 🧪 自动化测试

运行内置单元测试与子进程聚合代理的模拟集成测试：

```bash
cargo test
```

## 📄 许可证

本项目基于 MIT 许可证开源。
