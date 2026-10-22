//! D41: the real `cenango-mcp` program, spoken to the way Claude does (JSON-RPC over stdio),
//! against a real server on a free port.

use serde_json::{json, Value};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout};

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Mcp {
    _child: Child,
    stdin: ChildStdin,
    stdout: Lines<BufReader<ChildStdout>>,
    next: i64,
}

impl Mcp {
    fn start(base: &str, key: &str) -> Result<Mcp, Box<dyn std::error::Error>> {
        let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_cenango-mcp"))
            .env("CENANGO_API_URL", base)
            .env("CENANGO_API_KEY", key)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let stdin = child.stdin.take().ok_or("stdin")?;
        let stdout = BufReader::new(child.stdout.take().ok_or("stdout")?).lines();
        Ok(Mcp { _child: child, stdin, stdout, next: 1 })
    }

    async fn notify(&mut self, method: &str) -> TestResult {
        self.stdin.write_all(format!("{}\n", json!({"jsonrpc": "2.0", "method": method})).as_bytes()).await?;
        Ok(())
    }

    async fn rpc(&mut self, method: &str, params: Value) -> Result<Value, Box<dyn std::error::Error>> {
        let id = self.next;
        self.next += 1;
        let msg = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        self.stdin.write_all(format!("{msg}\n").as_bytes()).await?;
        let line = self.stdout.next_line().await?.ok_or("no answer")?;
        let v: Value = serde_json::from_str(&line)?;
        assert_eq!(v["id"], json!(id));
        Ok(v)
    }

    /// A tool call: (text, isError).
    async fn tool(&mut self, name: &str, args: Value) -> Result<(String, bool), Box<dyn std::error::Error>> {
        let r = self.rpc("tools/call", json!({"name": name, "arguments": args})).await?;
        let text = r["result"]["content"][0]["text"].as_str().unwrap_or("").to_string();
        Ok((text, r["result"]["isError"].as_bool().unwrap_or(false)))
    }
}

#[tokio::test]
async fn claude_reads_the_books_and_creates_only_drafts() -> TestResult {
    let dir = tempfile::tempdir()?;
    let config = app_core::config::AppConfig {
        data_dir: dir.path().join("data"),
        bind: "127.0.0.1:0".parse()?,
        lan_enabled: false,
        frontend_dir: None,
        lan_allowed: Vec::new(),
        lan_host_names: Vec::new(),
    };
    let server = api_server::start(&config).await?;
    let base = format!("http://{}", server.addr);
    let http = reqwest::Client::new();
    let first = http
        .post(format!("{base}/api/auth/first-run"))
        .json(&json!({"username": "owner", "password": "a-long-password"}))
        .send()
        .await?
        .json::<Value>()
        .await?;
    let token = first["accessToken"].as_str().ok_or("token")?.to_string();
    let call =
        |m: reqwest::Method, p: &str, b: Value| http.request(m, format!("{base}/api{p}")).bearer_auth(&token).json(&b);
    call(reqwest::Method::PATCH, "/settings", json!({"company.name": "Hill Tea Ltd", "region.currency": "LKR"}))
        .send()
        .await?;
    call(reqwest::Method::POST, "/customers", json!({"name": "ABC Traders"})).send().await?;
    let key = |write: bool| {
        let c = call(
            reqwest::Method::POST,
            "/api-keys",
            json!({"name": if write { "Claude" } else { "Claude reader" }, "canWrite": write}),
        );
        async move { c.send().await?.json::<Value>().await }
    };
    let write_key = key(true).await?["key"].as_str().ok_or("key")?.to_string();
    let read_key = key(false).await?["key"].as_str().ok_or("key")?.to_string();

    // The handshake, as Claude does it.
    let mut mcp = Mcp::start(&base, &write_key)?;
    let init = mcp.rpc("initialize", json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "1"}})).await?;
    assert_eq!(init["result"]["protocolVersion"], json!("2025-06-18"));
    assert_eq!(init["result"]["serverInfo"]["name"], json!("cenango-accounting"));
    mcp.notify("notifications/initialized").await?;
    let tools = mcp.rpc("tools/list", json!({})).await?;
    let names: Vec<String> = tools["result"]["tools"]
        .as_array()
        .ok_or("tools")?
        .iter()
        .filter_map(|t| t["name"].as_str().map(str::to_string))
        .collect();
    assert!(
        names.contains(&"run_report".to_string()) && names.contains(&"create_draft_invoice".to_string()),
        "{names:?}"
    );

    let (status, err) = mcp.tool("connection_status", json!({})).await?;
    assert!(!err && status.contains("Hill Tea Ltd") && status.contains("\"mayCreateDrafts\": true"), "{status}");

    // A draft invoice from plain words: the customer by name, the account by code.
    let (made, err) = mcp
        .tool(
            "create_draft_invoice",
            json!({"customer": "abc traders", "lines": [{"description": "Consulting", "quantity": 2, "unit_price": 1500, "account_code": "4000"}]}),
        )
        .await?;
    assert!(!err, "{made}");
    let made: Value = serde_json::from_str(&made)?;
    assert_eq!((made["status"].as_str(), made["total"].as_str()), (Some("DRAFT"), Some("3000.00")), "{made}");
    let number = made["draft"].as_str().ok_or("number")?.to_string();
    let (list, _) = mcp.tool("list_invoices", json!({"status": "DRAFT"})).await?;
    assert!(list.contains(&number) && list.contains("ABC Traders"), "{list}");
    let (one, err) = mcp.tool("get_invoice", json!({"number": number})).await?;
    assert!(!err && one.contains("Consulting"), "{one}");
    let (pl, err) = mcp
        .tool("run_report", json!({"report": "profit_loss", "date_from": "2026-01-01", "date_to": "2026-12-31"}))
        .await?;
    assert!(!err, "{pl}");
    let (bad, err) = mcp.tool("account_activity", json!({"account_code": "9999"})).await?;
    assert!(err && bad.contains("No account with code 9999"));
    let (unknown, err) = mcp.tool("delete_everything", json!({})).await?;
    assert!(err && unknown.contains("Unknown tool"));

    // Posting is never possible with a key, even a write key.
    let inv = http
        .get(format!("{base}/api/invoices?status=DRAFT"))
        .bearer_auth(&write_key)
        .send()
        .await?
        .json::<Value>()
        .await?;
    let id = inv["data"][0]["id"].as_str().ok_or("draft id")?;
    let post = http.post(format!("{base}/api/invoices/{id}/post")).bearer_auth(&write_key).send().await?;
    assert_eq!(post.status().as_u16(), 403);

    // A read-only key: no draft tools, and the server refuses a write anyway.
    let mut reader = Mcp::start(&base, &read_key)?;
    reader.rpc("initialize", json!({"protocolVersion": "2024-11-05"})).await?;
    let tools = reader.rpc("tools/list", json!({})).await?;
    assert!(!tools.to_string().contains("create_draft_invoice"));
    let (refused, err) = reader
        .tool(
            "create_draft_invoice",
            json!({"customer": "ABC Traders", "lines": [{"description": "x", "unit_price": 1}]}),
        )
        .await?;
    assert!(err && refused.contains("may only read"), "{refused}");
    // No key at all: a clear message, not a crash.
    let mut keyless = Mcp::start(&base, "")?;
    let (msg, err) = keyless.tool("list_invoices", json!({})).await?;
    assert!(err && msg.contains("CENANGO_API_KEY"), "{msg}");

    server.shutdown().await;
    Ok(())
}
