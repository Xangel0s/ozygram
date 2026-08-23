#[tokio::main]
async fn main() -> anyhow::Result<()> {
    ozymem_server::run_mcp_server().await
}
