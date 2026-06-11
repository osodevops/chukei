use anyhow::{Context, Result};
use clap::Args;
use std::path::PathBuf;

use chukei_core::config::Config;
use chukei_core::evidence;
use chukei_core::replay::{read_csv, simulate, ReplayOptions};

#[derive(Args)]
pub struct ReplayArgs {
    /// Snowflake query_history CSV export
    #[arg(long)]
    pub query_history: PathBuf,
    /// Path to chukei.yaml (defaults applied if absent)
    #[arg(long, short)]
    pub config: Option<PathBuf>,
    /// Write the projected-savings report (JSON) here
    #[arg(long, short)]
    pub output: Option<PathBuf>,
    /// Only report parse coverage and fingerprint dedup, skip savings simulation
    #[arg(long)]
    pub parse_only: bool,
    /// USD per Snowflake credit
    #[arg(long, default_value_t = 3.0)]
    pub usd_per_credit: f64,
    /// Write a signed evidence envelope (<output stem>.evidence.json).
    /// Signs with evidence.signing.private_key_path when configured,
    /// otherwise an ephemeral key (flagged inside the bundle).
    #[arg(long)]
    pub evidence: bool,
}

pub async fn run(args: ReplayArgs) -> Result<()> {
    let config = match &args.config {
        Some(path) => Config::from_file(path)?,
        None => Config::default(),
    };

    let corpus_bytes = std::fs::read(&args.query_history)
        .with_context(|| format!("cannot open {}", args.query_history.display()))?;
    let rows = read_csv(corpus_bytes.as_slice())?;

    let options = ReplayOptions {
        usd_per_credit: args.usd_per_credit,
        ..Default::default()
    };
    let report = simulate(&rows, &config, options);

    if args.parse_only {
        println!("rows:                     {}", report.parse.total_rows);
        println!("parsed:                   {}", report.parse.parsed);
        println!(
            "parse coverage:           {:.1}%",
            report.parse.coverage_pct
        );
        println!(
            "distinct hard fps:        {}",
            report.parse.distinct_hard_fingerprints
        );
        println!(
            "distinct soft fps:        {}",
            report.parse.distinct_soft_fingerprints
        );
        return Ok(());
    }

    let json = serde_json::to_string_pretty(&report)?;
    match &args.output {
        Some(path) => {
            std::fs::write(path, &json)
                .with_context(|| format!("cannot write {}", path.display()))?;
            println!("report written to {}", path.display());
        }
        None => println!("{json}"),
    }

    if args.evidence {
        let key_path = config
            .evidence
            .signing
            .private_key_path
            .as_ref()
            .filter(|_| config.evidence.signing.enabled)
            .map(std::path::PathBuf::from);
        let (key, ephemeral) = evidence::load_signing_key_or_default(key_path.as_deref())?;
        if ephemeral {
            eprintln!("note: no signing key configured; using an ephemeral demo-grade key");
        }

        let corpus_stem = args
            .query_history
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "corpus".into());
        let now = chrono::Utc::now();
        let bundle = evidence::EvidenceBundle {
            bundle_id: evidence::new_bundle_id("chukei-replay", &corpus_stem, now),
            kind: "replay-projection".into(),
            tool_version: env!("CARGO_PKG_VERSION").into(),
            signed_at: now,
            ephemeral_key: ephemeral,
            corpus: evidence::CorpusFacts {
                file: args.query_history.display().to_string(),
                rows: rows.len(),
                sha256_hex: evidence::sha256_hex(&corpus_bytes),
            },
            report: serde_json::to_value(&report)?,
        };
        let signed = evidence::sign(&bundle, &key)?;

        let envelope_path = match &args.output {
            Some(path) => path.with_extension("evidence.json"),
            None => PathBuf::from(format!(
                "{}-{}.evidence.json",
                bundle.bundle_id,
                evidence::short_hash(&signed)
            )),
        };
        std::fs::write(&envelope_path, serde_json::to_string_pretty(&signed)?)?;
        println!("evidence bundle:   {}", envelope_path.display());
        println!("bundle id:         {}", bundle.bundle_id);
        println!("public key:        {}", signed.public_key_b64);
        println!(
            "verify with:       chukei evidence verify --file {}",
            envelope_path.display()
        );
    }

    eprintln!();
    eprintln!("── projected savings ─────────────────────────────");
    eprintln!(
        "cache:     ${:.2} ({} hits, {:.1}% hit rate)",
        report.cache.saved_usd, report.cache.projected_hits, report.cache.hit_rate_pct
    );
    eprintln!(
        "router:    ${:.2} ({} routable queries)",
        report.router.saved_usd, report.router.routable_queries
    );
    eprintln!(
        "suspend:   ${:.2} ({:.1} idle hours)",
        report.suspend.saved_usd, report.suspend.idle_hours_saved
    );
    eprintln!(
        "rewrites:  {} queries flagged (not priced)",
        report.rewrite.queries_with_rewrites
    );
    eprintln!("total:     ${:.2}", report.projected_savings_usd);
    Ok(())
}
