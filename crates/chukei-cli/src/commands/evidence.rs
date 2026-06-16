use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use std::path::PathBuf;

use chukei_core::evidence;

#[derive(Args)]
pub struct EvidenceArgs {
    #[command(subcommand)]
    pub action: EvidenceAction,
}

#[derive(Subcommand)]
pub enum EvidenceAction {
    /// Generate a raw 32-byte Ed25519 signing key
    Keygen {
        /// Where to write the key (chmod it appropriately yourself)
        #[arg(long, short)]
        out: PathBuf,
    },
    /// Verify a signed evidence envelope
    Verify {
        /// Path to the .evidence.json envelope
        #[arg(long, short)]
        file: PathBuf,
        /// Require this exact public key (URL-safe base64), not just a
        /// self-consistent envelope
        #[arg(long)]
        public_key: Option<String>,
    },
}

pub async fn run(args: EvidenceArgs) -> Result<()> {
    match args.action {
        EvidenceAction::Keygen { out } => {
            if out.exists() {
                anyhow::bail!(
                    "{} already exists; refusing to overwrite a key",
                    out.display()
                );
            }
            let public_b64 = evidence::generate_key_file(&out)?;
            println!("signing key written: {}", out.display());
            println!("public key (b64url): {public_b64}");
            Ok(())
        }
        EvidenceAction::Verify { file, public_key } => {
            let raw = std::fs::read_to_string(&file)
                .with_context(|| format!("cannot read {}", file.display()))?;
            let signed: evidence::SignedEvidence =
                serde_json::from_str(&raw).context("not a SignedEvidence envelope")?;
            if let Some(expected) = public_key {
                if signed.public_key_b64 != expected {
                    anyhow::bail!(
                        "public key mismatch: envelope carries {}, expected {expected}",
                        signed.public_key_b64
                    );
                }
            }
            let bundle = evidence::verify(&signed)?;
            println!("VERIFIED  {}", bundle.bundle_id);
            println!("kind:        {}", bundle.kind);
            println!("tool:        chukei {}", bundle.tool_version);
            println!("signed at:   {}", bundle.signed_at);
            println!(
                "corpus:      {} ({} rows, sha256 {})",
                bundle.corpus.file,
                bundle.corpus.rows,
                &bundle.corpus.sha256_hex[..12]
            );
            let usd = bundle
                .report
                .get("projected_savings_usd")
                .or_else(|| bundle.report.get("total_avoided_usd"))
                .and_then(|v| v.as_f64());
            if let Some(usd) = usd {
                println!("savings:     ${usd:.2}");
            }
            if bundle.ephemeral_key {
                println!("note:        signed with an EPHEMERAL key (demo-grade, not compliance evidence)");
            }
            Ok(())
        }
    }
}
