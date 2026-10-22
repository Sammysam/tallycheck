//! D41: `cenango-mcp` — Cenango Accounting's MCP server (integrations plan, phase 5).
//!
//! Claude (Claude Desktop, Claude Code) starts this program and talks to it over stdin / stdout
//! (MCP: JSON-RPC 2.0, one message per line). It calls the running app on this PC with a key made
//! under Settings → Integrations → Claude:
//!
//! * `CENANGO_API_KEY` — the key (cak_…). Required.
//! * `CENANGO_API_URL` — the app, default `http://127.0.0.1:47800`.
//!
//! The key decides what Claude may do: read the books (always), and create DRAFT invoices, bills,
//! customers and vendors (only if the key allows it). Posting, paying, voiding and deleting stay in
//! the app — the server refuses them for keys. Nothing is written to stdout but MCP messages.

mod tools;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const SUPPORTED: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

fn reply(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn failure(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let api = tools::Api::from_env();
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut out = tokio::io::stdout();
    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => {
                let r = failure(&Value::Null, -32700, "Parse error");
                let _ = out.write_all(format!("{r}\n").as_bytes()).await;
                let _ = out.flush().await;
                continue;
            }
        };
        let id = msg.get("id").cloned();
        let method = msg["method"].as_str().unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        // Notifications (no id) get no answer.
        let Some(id) = id else { continue };
        let response = match method {
            "initialize" => {
                let asked = params["protocolVersion"].as_str().unwrap_or("");
                let version = if SUPPORTED.contains(&asked) { asked } else { SUPPORTED[0] };
                reply(
                    &id,
                    json!({
                        "protocolVersion": version,
                        "capabilities": { "tools": { "listChanged": false } },
                        "serverInfo": { "name": "cenango-accounting", "version": env!("CARGO_PKG_VERSION") },
                        "instructions": tools::INSTRUCTIONS,
                    }),
                )
            }
            "ping" => reply(&id, json!({})),
            "tools/list" => reply(&id, json!({ "tools": tools::list(&api).await })),
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
                let (text, is_error) = match tools::call(&api, name, &args).await {
                    Ok(v) => (serde_json::to_string_pretty(&v).unwrap_or_default(), false),
                    Err(e) => (e, true),
                };
                reply(&id, json!({ "content": [{ "type": "text", "text": text }], "isError": is_error }))
            }
            _ => failure(&id, -32601, "Method not found"),
        };
        if out.write_all(format!("{response}\n").as_bytes()).await.is_err() || out.flush().await.is_err() {
            break;
        }
    }
}
