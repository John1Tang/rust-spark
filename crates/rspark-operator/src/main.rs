use rspark_operator::{controller, install_tracing};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    install_tracing();
    tracing::info!("rspark-operator starting");

    let client = rspark_operator::client().await?;
    controller::run(client).await?;
    Ok(())
}
