//! End-to-end proxy tests against a mock Snowflake origin ("chukei-mock-sf",
//! PRD §21.2): a real HTTP round trip through the chukei router, exercising
//! passthrough, cache hit/miss, rewrite-in-flight, and upstream-down behaviour.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use axum::routing::{any, post};
use axum::{Json, Router};

use chukei_core::config::Config;
use chukei_core::wire::sf::{router, ProxyState};

#[derive(Default)]
struct MockStats {
    query_requests: AtomicUsize,
    last_sql: std::sync::Mutex<Option<String>>,
}

/// Minimal mock of the Snowflake REST origin.
async fn start_mock_snowflake(stats: Arc<MockStats>) -> SocketAddr {
    let app = Router::new()
        .route(
            "/session/v1/login-request",
            post(|| async {
                Json(serde_json::json!({
                    "success": true,
                    "data": { "token": "mock-token-123", "masterToken": "mock-master" }
                }))
            }),
        )
        .route(
            "/queries/v1/query-request",
            post(move |body: Json<serde_json::Value>| {
                let stats = stats.clone();
                async move {
                    stats.query_requests.fetch_add(1, Ordering::SeqCst);
                    let sql = body
                        .get("sqlText")
                        .and_then(|s| s.as_str())
                        .unwrap_or_default()
                        .to_string();
                    *stats.last_sql.lock().unwrap() = Some(sql);
                    Json(serde_json::json!({
                        "success": true,
                        "data": {
                            "rowset": [["1", "alpha"]],
                            "rowtype": [{"name": "ID"}, {"name": "NAME"}],
                            "total": 1
                        }
                    }))
                }
            }),
        )
        .fallback(any(|| async {
            Json(serde_json::json!({"success": true, "data": {}}))
        }));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

async fn start_proxy(upstream: SocketAddr, configure: impl FnOnce(&mut Config)) -> SocketAddr {
    start_proxy_with_state(upstream, configure).await.0
}

async fn start_proxy_with_state(
    upstream: SocketAddr,
    configure: impl FnOnce(&mut Config),
) -> (SocketAddr, Arc<ProxyState>) {
    let yaml = format!(
        r#"
upstream:
  snowflake:
    account: "mock"
    base_url_override: "http://{upstream}"
"#
    );
    let mut config = Config::from_yaml(&yaml).unwrap();
    configure(&mut config);
    let state = ProxyState::from_config(&config).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let serve_state = state.clone();
    tokio::spawn(async move { axum::serve(listener, router(serve_state)).await.unwrap() });
    (addr, state)
}

/// Drivers send a fresh requestId per logical request; mirror that here so
/// the retry-replay path only triggers in tests that exercise it on purpose.
static NEXT_REQUEST_ID: AtomicUsize = AtomicUsize::new(0);

async fn post_query(proxy: SocketAddr, sql: &str) -> serde_json::Value {
    let rid = NEXT_REQUEST_ID.fetch_add(1, Ordering::SeqCst);
    reqwest::Client::new()
        .post(format!(
            "http://{proxy}/queries/v1/query-request?requestId=test-{rid}"
        ))
        .header("Authorization", "Snowflake Token=\"mock-token-123\"")
        .json(&serde_json::json!({ "sqlText": sql, "sequenceId": 1 }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

#[tokio::test]
async fn login_and_query_pass_through() {
    let stats = Arc::new(MockStats::default());
    let upstream = start_mock_snowflake(stats.clone()).await;
    let proxy = start_proxy(upstream, |_| {}).await;

    let login: serde_json::Value = reqwest::Client::new()
        .post(format!("http://{proxy}/session/v1/login-request"))
        .json(&serde_json::json!({"data": {"LOGIN_NAME": "SARA", "CLIENT_APP_ID": "PythonConnector"}}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(login.pointer("/data/token").unwrap(), "mock-token-123");

    let result = post_query(proxy, "SELECT id, name FROM customers").await;
    assert_eq!(result.pointer("/data/total").unwrap(), 1);
    assert_eq!(stats.query_requests.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn repeated_query_served_from_cache() {
    let stats = Arc::new(MockStats::default());
    let upstream = start_mock_snowflake(stats.clone()).await;
    let proxy = start_proxy(upstream, |c| {
        c.plugins.cache.enabled = true;
    })
    .await;

    let first = post_query(
        proxy,
        "SELECT id, name FROM customers WHERE region = 'EMEA'",
    )
    .await;
    // Same query modulo whitespace/case → identical result identity → hit
    // without touching Snowflake.
    let second = post_query(
        proxy,
        "select id,   name from CUSTOMERS where region = 'EMEA'",
    )
    .await;

    assert_eq!(
        first.pointer("/data/rowset"),
        second.pointer("/data/rowset")
    );
    assert_eq!(
        stats.query_requests.load(Ordering::SeqCst),
        1,
        "second query must be a cache hit, upstream sees exactly one request"
    );
}

#[tokio::test]
async fn different_literals_never_share_a_cache_entry() {
    let stats = Arc::new(MockStats::default());
    let upstream = start_mock_snowflake(stats.clone()).await;
    let proxy = start_proxy(upstream, |c| {
        c.plugins.cache.enabled = true;
    })
    .await;

    post_query(proxy, "SELECT id FROM customers WHERE region = 'EMEA'").await;
    // Same shape, different literal: different results — must go upstream.
    post_query(proxy, "SELECT id FROM customers WHERE region = 'APAC'").await;
    assert_eq!(
        stats.query_requests.load(Ordering::SeqCst),
        2,
        "a literal change must never be served from another query's cache"
    );
}

#[tokio::test]
async fn non_deterministic_query_never_cached() {
    let stats = Arc::new(MockStats::default());
    let upstream = start_mock_snowflake(stats.clone()).await;
    let proxy = start_proxy(upstream, |c| {
        c.plugins.cache.enabled = true;
    })
    .await;

    post_query(proxy, "SELECT id, CURRENT_TIMESTAMP() FROM events").await;
    post_query(proxy, "SELECT id, CURRENT_TIMESTAMP() FROM events").await;
    assert_eq!(stats.query_requests.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn rewrite_applied_in_flight() {
    let stats = Arc::new(MockStats::default());
    let upstream = start_mock_snowflake(stats.clone()).await;
    let proxy = start_proxy(upstream, |c| {
        c.plugins.rewrite.enabled = true;
    })
    .await;

    post_query(
        proxy,
        "SELECT * FROM t WHERE region = 'a' OR region = 'b' OR region = 'c'",
    )
    .await;
    let sent = stats.last_sql.lock().unwrap().clone().unwrap();
    assert!(
        sent.contains("region IN ('a', 'b', 'c')"),
        "upstream saw: {sent}"
    );
}

#[tokio::test]
async fn unparseable_sql_passes_through_verbatim() {
    let stats = Arc::new(MockStats::default());
    let upstream = start_mock_snowflake(stats.clone()).await;
    let proxy = start_proxy(upstream, |c| {
        c.plugins.cache.enabled = true;
        c.plugins.rewrite.enabled = true;
    })
    .await;

    let weird = "CALL my_special_proc(:1, :2) -- not a SELECT";
    post_query(proxy, weird).await;
    let sent = stats.last_sql.lock().unwrap().clone().unwrap();
    assert_eq!(
        sent, weird,
        "F-001: unknown SQL must pass through byte-identical"
    );
}

/// Mock that responds slowly, so concurrent requests genuinely overlap.
async fn start_slow_mock(stats: Arc<MockStats>, delay_ms: u64) -> SocketAddr {
    let app = Router::new().fallback(any(move |body: String| {
        let stats = stats.clone();
        async move {
            stats.query_requests.fetch_add(1, Ordering::SeqCst);
            let sql = serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|v| v.get("sqlText").and_then(|s| s.as_str()).map(String::from))
                .unwrap_or_default();
            *stats.last_sql.lock().unwrap() = Some(sql);
            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
            Json(serde_json::json!({
                "success": true,
                "data": { "rowset": [["42"]], "total": 1 }
            }))
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

#[tokio::test]
async fn concurrent_identical_queries_coalesce_to_one_upstream_call() {
    let stats = Arc::new(MockStats::default());
    let upstream = start_slow_mock(stats.clone(), 300).await;
    let proxy = start_proxy(upstream, |_| {}).await; // cache off; coalescing is on by default

    let mut handles = Vec::new();
    for _ in 0..10 {
        handles.push(tokio::spawn(post_query(
            proxy,
            "SELECT region, SUM(amount) FROM marts.revenue GROUP BY region",
        )));
    }
    for h in handles {
        let body = h.await.unwrap();
        assert_eq!(
            body.pointer("/data/rowset/0/0").and_then(|v| v.as_str()),
            Some("42"),
            "every coalesced follower must receive the leader's result"
        );
    }
    assert_eq!(
        stats.query_requests.load(Ordering::SeqCst),
        1,
        "10 concurrent identical queries must produce exactly 1 upstream execution"
    );
}

#[tokio::test]
async fn different_queries_do_not_coalesce() {
    let stats = Arc::new(MockStats::default());
    let upstream = start_slow_mock(stats.clone(), 200).await;
    let proxy = start_proxy(upstream, |_| {}).await;

    let a = tokio::spawn(post_query(proxy, "SELECT a FROM t WHERE x = 'p'"));
    let b = tokio::spawn(post_query(proxy, "SELECT a FROM t WHERE x = 'q'"));
    a.await.unwrap();
    b.await.unwrap();
    assert_eq!(
        stats.query_requests.load(Ordering::SeqCst),
        2,
        "different literals → different results → must not coalesce"
    );
}

#[tokio::test]
async fn non_deterministic_queries_do_not_coalesce() {
    let stats = Arc::new(MockStats::default());
    let upstream = start_slow_mock(stats.clone(), 200).await;
    let proxy = start_proxy(upstream, |_| {}).await;

    let a = tokio::spawn(post_query(proxy, "SELECT UUID_STRING() FROM t"));
    let b = tokio::spawn(post_query(proxy, "SELECT UUID_STRING() FROM t"));
    a.await.unwrap();
    b.await.unwrap();
    assert_eq!(stats.query_requests.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn gzipped_request_bodies_are_inspected_and_rewritten() {
    // snowflake-connector-python gzips every POST body. The mock here
    // decompresses what it receives, so we can assert chukei (a) decoded the
    // body to inspect it and (b) re-encoded the rewritten body as gzip.
    let stats = Arc::new(MockStats::default());
    let mock_stats = stats.clone();
    let app = Router::new().fallback(any(
        move |headers: axum::http::HeaderMap, body: axum::body::Bytes| {
            let stats = mock_stats.clone();
            async move {
                stats.query_requests.fetch_add(1, Ordering::SeqCst);
                let raw = if headers
                    .get("content-encoding")
                    .is_some_and(|v| v.as_bytes() == b"gzip")
                {
                    let mut out = Vec::new();
                    std::io::Read::read_to_end(
                        &mut flate2::read::GzDecoder::new(body.as_ref()),
                        &mut out,
                    )
                    .unwrap();
                    out
                } else {
                    body.to_vec()
                };
                let sql = serde_json::from_slice::<serde_json::Value>(&raw)
                    .ok()
                    .and_then(|v| v.get("sqlText").and_then(|s| s.as_str()).map(String::from))
                    .unwrap_or_default();
                *stats.last_sql.lock().unwrap() = Some(sql);
                Json(serde_json::json!({"success": true, "data": {"total": 1}}))
            }
        },
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let proxy = start_proxy(upstream, |c| {
        c.plugins.rewrite.enabled = true;
    })
    .await;

    let payload =
        serde_json::json!({"sqlText": "SELECT a FROM t WHERE x = 'p' OR x = 'q' OR x = 'r'"});
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut encoder, payload.to_string().as_bytes()).unwrap();
    let gz_body = encoder.finish().unwrap();

    let resp: serde_json::Value = reqwest::Client::new()
        .post(format!("http://{proxy}/queries/v1/query-request"))
        .header("content-type", "application/json")
        .header("content-encoding", "gzip")
        .body(gz_body)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(resp.pointer("/data/total").unwrap(), 1);

    let sent = stats.last_sql.lock().unwrap().clone().unwrap();
    assert!(
        sent.contains("x IN ('p', 'q', 'r')"),
        "gzipped body must be decoded, rewritten, and re-gzipped; upstream saw: {sent:?}"
    );
}

#[tokio::test]
async fn tls_terminated_query_round_trip() {
    let stats = Arc::new(MockStats::default());
    let upstream = start_mock_snowflake(stats.clone()).await;

    // Customer-owned cert, self-signed for the test.
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let cert_path = dir.path().join("tls.crt");
    let key_path = dir.path().join("tls.key");
    std::fs::write(&cert_path, cert.pem()).unwrap();
    std::fs::write(&key_path, signing_key.serialize_pem()).unwrap();

    let yaml = format!(
        r#"
listen:
  bind: "127.0.0.1:0"
  tls:
    cert: "{}"
    key: "{}"
upstream:
  snowflake:
    account: "mock"
    base_url_override: "http://{upstream}"
plugins:
  rewrite:
    enabled: true
"#,
        cert_path.display(),
        key_path.display()
    );
    let config = Config::from_yaml(&yaml).unwrap();
    let handle = axum_server::Handle::new();
    let serve_handle = handle.clone();
    tokio::spawn(async move {
        chukei_core::wire::sf::serve_with_handle(&config, serve_handle)
            .await
            .unwrap();
    });
    // Fail loudly if the server task dies before binding (a bare
    // handle.listening() would hang the whole test run instead).
    let addr = tokio::time::timeout(std::time::Duration::from_secs(10), handle.listening())
        .await
        .expect("server did not start listening within 10s")
        .unwrap();

    // Driver-side trust of the customer cert; OCSP/pinning concerns are
    // documented (PRD §8.5), so the test client just trusts our test CA.
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap();
    let result: serde_json::Value = client
        .post(format!(
            "https://localhost:{}/queries/v1/query-request",
            addr.port()
        ))
        .json(&serde_json::json!({
            "sqlText": "SELECT * FROM t WHERE x = 'a' OR x = 'b' OR x = 'c'"
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(result.pointer("/data/total").unwrap(), 1);
    let sent = stats.last_sql.lock().unwrap().clone().unwrap();
    assert!(
        sent.contains("x IN ('a', 'b', 'c')"),
        "rewrite must work through TLS too, upstream saw: {sent}"
    );
}

#[tokio::test]
async fn session_facts_survive_token_renewal() {
    let stats = Arc::new(MockStats::default());
    let upstream_addr = {
        // Mock with login + token-request renewal.
        let app = Router::new()
            .route(
                "/session/v1/login-request",
                post(|| async {
                    Json(serde_json::json!({"success": true, "data": {"token": "token-OLD"}}))
                }),
            )
            .route(
                "/session/token-request",
                post(|| async {
                    Json(
                        serde_json::json!({"success": true, "data": {"sessionToken": "token-NEW"}}),
                    )
                }),
            )
            .fallback(any(|| async {
                Json(serde_json::json!({"success": true, "data": {}}))
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        addr
    };
    let _ = stats;
    let (proxy, state) = start_proxy_with_state(upstream_addr, |_| {}).await;

    let client = reqwest::Client::new();
    client
        .post(format!("http://{proxy}/session/v1/login-request"))
        .json(&serde_json::json!({"data": {"LOGIN_NAME": "SARA"}}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        state.session_user_for_token("token-OLD").as_deref(),
        Some("SARA")
    );

    client
        .post(format!("http://{proxy}/session/token-request"))
        .json(&serde_json::json!({"oldSessionToken": "token-OLD", "requestType": "RENEW"}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        state.session_user_for_token("token-NEW").as_deref(),
        Some("SARA"),
        "session facts must follow the renewed token"
    );
    assert_eq!(state.session_user_for_token("token-OLD"), None);
}

#[tokio::test]
async fn driver_retry_with_same_request_id_replays_response() {
    let stats = Arc::new(MockStats::default());
    let upstream = start_mock_snowflake(stats.clone()).await;
    let proxy = start_proxy(upstream, |_| {}).await;

    let url = format!("http://{proxy}/queries/v1/query-request?requestId=RID-1");
    let body = serde_json::json!({"sqlText": "SELECT a FROM t"});
    let client = reqwest::Client::new();
    for _ in 0..3 {
        let resp: serde_json::Value = client
            .post(&url)
            .header("Authorization", "Snowflake Token=\"tok\"")
            .json(&body)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(resp.pointer("/data/total").unwrap(), 1);
    }
    assert_eq!(
        stats.query_requests.load(Ordering::SeqCst),
        1,
        "retries with the same requestId must replay, not re-execute"
    );

    // A different requestId is a different logical request.
    client
        .post(format!(
            "http://{proxy}/queries/v1/query-request?requestId=RID-2"
        ))
        .header("Authorization", "Snowflake Token=\"tok\"")
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(stats.query_requests.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn observed_write_invalidates_cache_for_touched_tables() {
    let stats = Arc::new(MockStats::default());
    let upstream = start_mock_snowflake(stats.clone()).await;
    let proxy = start_proxy(upstream, |c| {
        c.plugins.cache.enabled = true;
    })
    .await;

    // Fill, hit, write, then the next read must go upstream again.
    post_query(proxy, "SELECT a FROM analytics.marts.revenue").await; // upstream #1 (fill)
    post_query(proxy, "SELECT a FROM analytics.marts.revenue").await; // cache hit
    assert_eq!(stats.query_requests.load(Ordering::SeqCst), 1);
    post_query(proxy, "INSERT INTO analytics.marts.revenue VALUES (1)").await; // upstream #2
    post_query(proxy, "SELECT a FROM analytics.marts.revenue").await; // upstream #3 (invalidated)
    assert_eq!(
        stats.query_requests.load(Ordering::SeqCst),
        3,
        "a write through the proxy must invalidate cached reads of that table"
    );
}

#[tokio::test]
async fn blame_mode_detects_and_evicts_stale_entries() {
    // Mock whose answer CHANGES on every call: any cached entry is
    // immediately stale, so a sampled blame check must catch it.
    let counter = Arc::new(AtomicUsize::new(0));
    let mock_counter = counter.clone();
    let app = Router::new().fallback(any(move || {
        let counter = mock_counter.clone();
        async move {
            let n = counter.fetch_add(1, Ordering::SeqCst);
            Json(serde_json::json!({
                "success": true,
                "data": { "rowset": [[n.to_string()]], "total": 1 }
            }))
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let (proxy, state) = start_proxy_with_state(upstream, |c| {
        c.plugins.cache.enabled = true;
        c.plugins.cache.blame_sample_rate = 1.0; // check every hit
    })
    .await;

    post_query(proxy, "SELECT a FROM t").await; // fill (mock answer 0)
    post_query(proxy, "SELECT a FROM t").await; // hit + blame check (answer 1 ≠ 0)
    tokio::time::sleep(std::time::Duration::from_millis(300)).await; // let blame task land

    let metrics = state.metrics.render();
    assert!(
        metrics.contains("chukei_cache_blame_mismatches_total 1"),
        "blame mismatch must be counted; metrics:\n{metrics}"
    );
    // Entry evicted → next query goes upstream again (a fresh fill).
    let before = counter.load(Ordering::SeqCst);
    post_query(proxy, "SELECT a FROM t").await;
    assert!(
        counter.load(Ordering::SeqCst) > before,
        "stale entry must be evicted after a blame mismatch"
    );
}

#[tokio::test]
async fn cache_hits_land_in_the_savings_ledger() {
    let stats = Arc::new(MockStats::default());
    let upstream = start_mock_snowflake(stats.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("savings.db");
    let db_path_str = db_path.display().to_string();
    let (proxy, state) = start_proxy_with_state(upstream, |c| {
        c.plugins.cache.enabled = true;
        c.savings.db_path = Some(db_path_str.clone());
    })
    .await;

    post_query(proxy, "SELECT a FROM analytics.marts.revenue").await; // fill
    post_query(proxy, "SELECT a FROM analytics.marts.revenue").await; // hit → saving
    tokio::time::sleep(std::time::Duration::from_millis(200)).await; // ledger write is async

    let metrics = state.metrics.render();
    assert!(
        metrics.contains("chukei_saved_usd_total"),
        "saved_usd metric must exist:\n{metrics}"
    );

    let ledger = chukei_core::savings::Ledger::open(
        &db_path,
        chukei_core::savings::Pricing::from_config(&chukei_core::config::SavingsConfig::default()),
    )
    .unwrap();
    let report = ledger.report(0, u64::MAX / 2).unwrap();
    assert_eq!(report.total_events, 1, "exactly one cache hit recorded");
    assert_eq!(report.by_kind["cache_hit"].events, 1);
}

#[tokio::test]
async fn enforce_mode_suspends_idle_warehouse_via_service_session() {
    // Mock origin that accepts the service login and records statements.
    let statements: Arc<std::sync::Mutex<Vec<String>>> = Arc::default();
    let mock_statements = statements.clone();
    let app = Router::new()
        .route(
            "/session/v1/login-request",
            post(|| async {
                Json(serde_json::json!({"success": true, "data": {"token": "svc-token"}}))
            }),
        )
        .route(
            "/queries/v1/query-request",
            post(move |body: Json<serde_json::Value>| {
                let statements = mock_statements.clone();
                async move {
                    let sql = body
                        .get("sqlText")
                        .and_then(|s| s.as_str())
                        .unwrap_or_default()
                        .to_string();
                    statements.lock().unwrap().push(sql);
                    Json(serde_json::json!({"success": true, "data": {}}))
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let (_, state) = start_proxy_with_state(upstream, |c| {
        c.plugins.suspend.enabled = true;
        c.plugins.suspend.mode = chukei_core::config::SuspendMode::Enforce;
        c.plugins.suspend.role = Some("CHUKEI_SUSPENDER".into());
        c.plugins.suspend.sweep_interval_secs = 1;
        c.service_account.user = Some("svc".into());
        c.service_account.password = Some("secret".into());
        c.service_account.role = Some("CHUKEI_SUSPENDER".into());
    })
    .await;

    // Warm the model with sparse traffic (10-min gaps). The last arrival is
    // 2 min ago: recently enough that ~8 min of billable idle remains —
    // that's the window where suspending beats the 60 s resume penalty.
    let suspend = state.suspend_plugin().expect("suspend plugin enabled");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let mut ts = now - 120_000 - 29 * 600_000;
    for _ in 0..30 {
        suspend.model.record_arrival("IDLE_WH", ts);
        ts += 600_000;
    }
    assert!(
        suspend.model.recommend("IDLE_WH", now).is_some(),
        "model should recommend suspending IDLE_WH"
    );

    // The sweeper isn't started by router() — start it the way `serve` does.
    chukei_core::wire::sf::spawn_sweeper(state.clone());
    tokio::time::sleep(std::time::Duration::from_millis(2500)).await;

    let seen = statements.lock().unwrap().clone();
    assert!(
        seen.iter()
            .any(|s| s.contains("ALTER WAREHOUSE \"IDLE_WH\" SUSPEND")),
        "service session must execute the suspend; statements seen: {seen:?}"
    );
    assert!(
        seen.iter().any(|s| s.contains("USE ROLE CHUKEI_SUSPENDER")),
        "service session must assume the gated role first; statements: {seen:?}"
    );
    let metrics = state.metrics.render();
    assert!(
        metrics.contains("chukei_suspends_executed_total 1"),
        "exactly one suspend (cooldown prevents repeats):\n{metrics}"
    );
}

#[tokio::test]
async fn upstream_down_returns_503() {
    // Point at a port nothing listens on.
    let dead: SocketAddr = "127.0.0.1:1".parse().unwrap();
    let proxy = start_proxy(dead, |_| {}).await;
    let status = reqwest::Client::new()
        .post(format!("http://{proxy}/queries/v1/query-request"))
        .json(&serde_json::json!({ "sqlText": "SELECT 1 FROM t" }))
        .send()
        .await
        .unwrap()
        .status();
    assert_eq!(status, 503);
}
