use anyhow::Result;
use clap::{Args, Subcommand};
use std::path::PathBuf;

#[derive(Args)]
pub struct ValidateArgs {
    #[command(subcommand)]
    pub target: ValidateTarget,
}

#[derive(Subcommand)]
pub enum ValidateTarget {
    /// Validate a configuration file
    Config {
        #[arg(long, short)]
        file: PathBuf,
    },
}

pub async fn run(args: ValidateArgs) -> Result<()> {
    match args.target {
        ValidateTarget::Config { file } => {
            chukei_core::config::Config::from_file(&file)?;
            println!("{}: valid", file.display());
            Ok(())
        }
    }
}
