//! `cenango-mcp.exe`: the name of TallyCheck's MCP server before the rename, kept so existing
//! Claude connections keep working. The same program as `tallycheck-mcp.exe`.

#[tokio::main(flavor = "current_thread")]
async fn main() {
    mcp::run().await;
}
