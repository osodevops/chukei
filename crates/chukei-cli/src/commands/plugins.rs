use anyhow::Result;
use clap::{Args, Subcommand};

use chukei_core::plugin::registry::catalog;

#[derive(Args)]
pub struct PluginsArgs {
    #[command(subcommand)]
    pub action: PluginsAction,
}

#[derive(Subcommand)]
pub enum PluginsAction {
    /// List built-in plugins
    List,
    /// Describe one plugin
    Describe { name: String },
}

pub async fn run(args: PluginsArgs) -> Result<()> {
    match args.action {
        PluginsAction::List => {
            println!("{:<12} {:<7} {:<7} SUMMARY", "NAME", "ORDER", "STATUS");
            for p in catalog() {
                println!(
                    "{:<12} {:<7} {:<7} {}",
                    p.name, p.order, p.status, p.summary
                );
            }
        }
        PluginsAction::Describe { name } => match catalog().into_iter().find(|p| p.name == name) {
            Some(p) => {
                println!("name:    {}", p.name);
                println!("order:   {}", p.order);
                println!("status:  {}", p.status);
                println!("summary: {}", p.summary);
            }
            None => anyhow::bail!("unknown plugin '{name}' (try `chukei plugins list`)"),
        },
    }
    Ok(())
}
