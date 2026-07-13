use rspark_operator::{controller, install_tracing};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // rustls 0.23 requires explicit crypto-provider selection when no
    // single "ring" / "aws-lc-rs" feature gets unified into the build.
    // The k3s cluster runs on linux/musl where neither feature gets
    // pulled in by default, so kube's TLS handshakes panic on first
    // use. Installing the default ring provider once at startup covers
    // every cross-compile target.
    let _ = rustls::crypto::ring::default_provider().install_default();

    install_tracing();
    tracing::info!("rspark-operator starting");

    let client = rspark_operator::client().await?;
    controller::run(client).await?;
    Ok(())
}
