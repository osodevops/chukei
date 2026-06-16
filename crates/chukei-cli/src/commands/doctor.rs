use anyhow::Result;
use clap::Args;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use chukei_core::config::Config;

#[derive(Args)]
pub struct DoctorArgs {
    /// Path to chukei.yaml
    #[arg(long, short)]
    pub config: PathBuf,
    /// Also probe the service-account login (consumes one Snowflake login)
    #[arg(long)]
    pub probe_login: bool,
}

struct Check {
    name: &'static str,
    ok: bool,
    detail: String,
}

fn check(name: &'static str, result: std::result::Result<String, String>) -> Check {
    match result {
        Ok(detail) => Check {
            name,
            ok: true,
            detail,
        },
        Err(detail) => Check {
            name,
            ok: false,
            detail,
        },
    }
}

pub async fn run(args: DoctorArgs) -> Result<()> {
    let mut checks = Vec::new();

    // 1. Config parses + validates.
    let config = match Config::from_file(&args.config) {
        Ok(c) => {
            checks.push(check(
                "config",
                Ok(format!("{} valid", args.config.display())),
            ));
            c
        }
        Err(e) => {
            print_checks(&[check("config", Err(e.to_string()))]);
            std::process::exit(2);
        }
    };

    // 2. TLS cert/key load, when configured.
    if let Some(tls) = &config.listen.tls {
        let loaded =
            axum_server::tls_rustls::RustlsConfig::from_pem_file(&tls.cert, &tls.key).await;
        checks.push(check(
            "tls",
            loaded
                .map(|_| format!("cert {} + key load OK", tls.cert))
                .map_err(|e| format!("cannot load cert/key: {e}")),
        ));
    } else {
        checks.push(check(
            "tls",
            Ok("not configured (plain HTTP — drivers need TLS in production)".into()),
        ));
    }

    // 3. Upstream DNS + TCP + TLS handshake.
    match &config.upstream.snowflake {
        Some(sf) => {
            let base = sf.base_url();
            let started = Instant::now();
            let client = reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(15))
                .build()?;
            // Any HTTP status proves DNS + TCP + TLS; Snowflake answers
            // 4xx on a bare GET, which is fine.
            let result = client.get(format!("{base}/")).send().await;
            checks.push(check(
                "upstream",
                result
                    .map(|r| {
                        format!(
                            "{base} reachable (HTTP {} in {} ms)",
                            r.status(),
                            started.elapsed().as_millis()
                        )
                    })
                    .map_err(|e| format!("{base} unreachable: {e}")),
            ));
        }
        None => checks.push(check(
            "upstream",
            Err("upstream.snowflake not configured".into()),
        )),
    }

    // 4. Listener port available (bind + release).
    let bind_check = tokio::net::TcpListener::bind(&config.listen.bind).await;
    checks.push(check(
        "listen",
        bind_check
            .map(|_| format!("{} bindable", config.listen.bind))
            .map_err(|e| format!("cannot bind {}: {e}", config.listen.bind)),
    ));

    // 5. Savings ledger writable.
    if config.savings.enabled {
        let path = config
            .savings
            .db_path
            .clone()
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("chukei-savings.db"));
        let result = chukei_core::savings::Ledger::open(
            &path,
            chukei_core::savings::Pricing::from_config(&config.savings),
        );
        checks.push(check(
            "savings",
            result
                .map(|_| format!("ledger writable at {}", path.display()))
                .map_err(|e| e.to_string()),
        ));
    }

    // 6. Cache persistence dir writable.
    if let Some(path) = config
        .plugins
        .cache
        .persist_path
        .as_ref()
        .filter(|_| config.plugins.cache.enabled)
    {
        use chukei_core::cache::CacheStore as _;
        let result = chukei_core::cache::disk::DiskStore::open(path, 1);
        checks.push(check(
            "cache",
            result
                .map(|s| format!("persistent dir OK ({} entries)", s.len()))
                .map_err(|e| e.to_string()),
        ));
    }

    // 7. Optional service-account login probe.
    if args.probe_login {
        match chukei_core::wire::sf::service::ServiceSession::from_config(&config)? {
            Some(service) => {
                let result = service.execute("SELECT 1").await;
                checks.push(check(
                    "service",
                    result
                        .map(|_| "service-account login + SELECT 1 OK".to_string())
                        .map_err(|e| e.to_string()),
                ));
            }
            None => checks.push(check(
                "service",
                Err("service_account not configured".into()),
            )),
        }
    }

    print_checks(&checks);
    if checks.iter().any(|c| !c.ok) {
        std::process::exit(3);
    }
    Ok(())
}

fn print_checks(checks: &[Check]) {
    for c in checks {
        println!(
            "{} {:<10} {}",
            if c.ok { "✓" } else { "✗" },
            c.name,
            c.detail
        );
    }
}
