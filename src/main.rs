//! eco — voice sessions with an AI beside them, for Omarchy.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    eco::run().await
}
