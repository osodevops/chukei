use anyhow::{bail, Context, Result};
use clap::Args;
use std::time::Duration;

#[derive(Args)]
pub struct HealthcheckArgs {
    /// Health endpoint to probe
    #[arg(long, default_value = "http://127.0.0.1:9090/healthz")]
    pub url: String,
    /// Request timeout in milliseconds
    #[arg(long, default_value_t = 2000)]
    pub timeout_ms: u64,
}

pub async fn run(args: HealthcheckArgs) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(args.timeout_ms.max(1)))
        .build()
        .context("cannot build healthcheck HTTP client")?;

    let response = client
        .get(&args.url)
        .send()
        .await
        .with_context(|| format!("healthcheck request failed: {}", args.url))?;
    let status = response.status();
    if !status.is_success() {
        bail!("healthcheck failed: {} returned {}", args.url, status);
    }

    println!("{} ok ({})", args.url, status);
    Ok(())
}
