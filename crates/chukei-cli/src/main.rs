use clap::{Parser, Subcommand};

mod commands;

#[derive(Parser)]
#[command(
    name = "chukei",
    version,
    about = "Transparent Snowflake/Databricks query proxy"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the proxy daemon
    Up(commands::up::UpArgs),
    /// Health-check a configuration and its upstream connectivity
    Doctor(commands::doctor::DoctorArgs),
    /// Validate configuration or cache state
    Validate(commands::validate::ValidateArgs),
    /// Simulate plugin savings against a query_history dump
    Replay(commands::replay::ReplayArgs),
    /// List and describe plugins
    Plugins(commands::plugins::PluginsArgs),
    /// Signed evidence bundles: keygen, verify
    Evidence(commands::evidence::EvidenceArgs),
    /// Realized savings recorded by the running proxy
    Savings(commands::savings::SavingsArgs),
    /// Generate shell completions (bash, zsh, fish, …)
    Completions {
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();
    let result = match cli.command {
        Commands::Up(args) => commands::up::run(args).await,
        Commands::Doctor(args) => commands::doctor::run(args).await,
        Commands::Validate(args) => commands::validate::run(args).await,
        Commands::Replay(args) => commands::replay::run(args).await,
        Commands::Plugins(args) => commands::plugins::run(args).await,
        Commands::Evidence(args) => commands::evidence::run(args).await,
        Commands::Savings(args) => commands::savings::run(args).await,
        Commands::Completions { shell } => {
            use clap::CommandFactory;
            clap_complete::generate(shell, &mut Cli::command(), "chukei", &mut std::io::stdout());
            Ok(())
        }
    };

    if let Err(e) = result {
        eprintln!("error: {e:#}");
        let code = e
            .downcast_ref::<chukei_core::Error>()
            .map(|e| e.exit_code())
            .unwrap_or(1);
        std::process::exit(code);
    }
}
