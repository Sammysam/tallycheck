//! `tallycheck-mcp.exe`: TallyCheck's MCP server for Claude (see the `mcp` library).

#[tokio::main(flavor = "current_thread")]
async fn main() {
    mcp::run().await;
}
