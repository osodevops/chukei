use anyhow::Result;
use clap::Args;
use std::path::PathBuf;

#[derive(Args)]
pub struct UpArgs {
    /// Path to chukei.yaml
    #[arg(long, short)]
    pub config: PathBuf,
}

pub async fn run(args: UpArgs) -> Result<()> {
    let config = chukei_core::config::Config::from_file(&args.config)?;
    chukei_core::wire::sf::serve(&config).await?;
    Ok(())
}
