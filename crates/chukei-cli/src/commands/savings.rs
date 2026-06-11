use anyhow::{Context, Result};
use clap::Args;
use std::path::PathBuf;

use chukei_core::config::Config;
use chukei_core::evidence;
use chukei_core::savings::{Ledger, Pricing};

#[derive(Args)]
pub struct SavingsArgs {
    /// Path to chukei.yaml (used for savings db path, pricing, signing key)
    #[arg(long, short)]
    pub config: Option<PathBuf>,
    /// Savings SQLite db (overrides config)
    #[arg(long)]
    pub db: Option<PathBuf>,
    /// Window, e.g. 24h, 7d, 30d
    #[arg(long, default_value = "7d")]
    pub since: String,
    /// table | json
    #[arg(long, default_value = "table")]
    pub format: String,
    /// Write a signed evidence envelope of this report here
    #[arg(long)]
    pub evidence: Option<PathBuf>,
}

fn parse_window(s: &str) -> Result<u64> {
    let s = s.trim();
    let (num, unit) = s.split_at(s.len().saturating_sub(1));
    let n: u64 = num
        .parse()
        .with_context(|| format!("invalid window '{s}'"))?;
    let ms = match unit {
        "m" => n * 60_000,
        "h" => n * 3_600_000,
        "d" => n * 86_400_000,
        _ => anyhow::bail!("invalid window '{s}' (use e.g. 90m, 24h, 7d)"),
    };
    Ok(ms)
}

pub async fn run(args: SavingsArgs) -> Result<()> {
    let config = match &args.config {
        Some(path) => Config::from_file(path)?,
        None => Config::default(),
    };
    let db_path = args
        .db
        .clone()
        .or_else(|| config.savings.db_path.clone().map(PathBuf::from))
        .unwrap_or_else(|| std::env::temp_dir().join("chukei-savings.db"));
    if !db_path.exists() {
        anyhow::bail!(
            "savings db not found at {} — has the proxy been running with savings.enabled?",
            db_path.display()
        );
    }

    let ledger = Ledger::open(&db_path, Pricing::from_config(&config.savings))?;
    let now_ms = chrono::Utc::now().timestamp_millis() as u64;
    let since_ms = now_ms.saturating_sub(parse_window(&args.since)?);
    let report = ledger.report(since_ms, now_ms)?;

    match args.format.as_str() {
        "json" => println!("{}", serde_json::to_string_pretty(&report)?),
        _ => {
            println!(
                "── realized savings (last {}) ───────────────────",
                args.since
            );
            println!(
                "{:<14} {:>8} {:>14} {:>12}",
                "KIND", "EVENTS", "CREDITS", "USD"
            );
            for (kind, agg) in &report.by_kind {
                println!(
                    "{:<14} {:>8} {:>14.4} {:>12.2}",
                    kind, agg.events, agg.avoided_credits, agg.avoided_usd
                );
            }
            println!(
                "{:<14} {:>8} {:>14.4} {:>12.2}",
                "TOTAL",
                report.total_events,
                report.total_avoided_credits,
                report.total_avoided_usd
            );
            if !report.by_team.is_empty() {
                println!("\nby team:");
                for (team, usd) in &report.by_team {
                    println!("  {team:<20} ${usd:.2}");
                }
            }
            println!("\nmethodology: {}", report.methodology);
        }
    }

    if let Some(out) = &args.evidence {
        let key_path = config
            .evidence
            .signing
            .private_key_path
            .as_ref()
            .filter(|_| config.evidence.signing.enabled)
            .map(PathBuf::from);
        let (key, ephemeral) = evidence::load_signing_key_or_default(key_path.as_deref())?;
        if ephemeral {
            eprintln!("note: no signing key configured; using an ephemeral demo-grade key");
        }
        let db_bytes = std::fs::read(&db_path)?;
        let now = chrono::Utc::now();
        let bundle = evidence::EvidenceBundle {
            bundle_id: evidence::new_bundle_id("chukei-savings", &args.since, now),
            kind: "savings-ledger".into(),
            tool_version: env!("CARGO_PKG_VERSION").into(),
            signed_at: now,
            ephemeral_key: ephemeral,
            corpus: evidence::CorpusFacts {
                file: db_path.display().to_string(),
                rows: report.total_events as usize,
                sha256_hex: evidence::sha256_hex(&db_bytes),
            },
            report: serde_json::to_value(&report)?,
        };
        let signed = evidence::sign(&bundle, &key)?;
        std::fs::write(out, serde_json::to_string_pretty(&signed)?)?;
        println!("\nsigned evidence: {}", out.display());
    }
    Ok(())
}
