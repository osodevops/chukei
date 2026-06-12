//! `chukei-wire-sf` — the Snowflake HTTPS wire shim (PRD §8).
//!
//! Snowflake drivers speak HTTPS against the account's REST endpoint. We
//! reverse-proxy every request verbatim (F-001: 100 % passthrough), and
//! intercept exactly one: `POST /queries/v1/query-request`, where the SQL
//! lives. There the plugin bus may serve from cache, rewrite the SQL, or
//! annotate — and on any internal failure the original request passes
//! through untouched. Auth packets are never inspected or persisted.

pub mod service;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::body::{to_bytes, Body};
use axum::extract::{Request, State};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::Router;
use hyper::{HeaderMap, Method, StatusCode, Uri};

use crate::cache::CachePlugin;
use crate::config::Config;
use crate::error::{Error, Result};
use crate::plugin::bus::PluginBus;
use crate::plugin::registry::build_bus;
use crate::plugin::{QueryContext, ResultSnapshot, Session};
use crate::sql::analyze;

/// Request/response bodies above this size are never buffered for
/// inspection; they stream through. 64 MiB matches Snowflake's inline
/// rowset ceiling with headroom.
const MAX_INSPECT_BYTES: usize = 64 * 1024 * 1024;

/// Status + body + canonical wall-clock (ms) of a completed upstream
/// response, shared with coalesced followers.
type CoalescedResponse = Arc<(u16, Vec<u8>, u64)>;

/// Drivers reuse the same `requestId` query parameter when they retry a
/// logical request (incrementing `retryCount`). For read-only deterministic
/// queries we replay the completed response instead of re-executing.
const DEDUP_TTL_MS: u64 = 60_000;
const DEDUP_CAP: usize = 1024;

#[derive(Default)]
struct RecentResponses {
    /// key → (status, body, canonical elapsed ms, inserted at ms)
    map: HashMap<String, (u16, Vec<u8>, u64, u64)>,
    order: std::collections::VecDeque<String>,
}

impl RecentResponses {
    fn get(&self, key: &str, now_ms: u64) -> Option<(u16, Vec<u8>, u64)> {
        match self.map.get(key) {
            Some((status, body, elapsed, at)) if now_ms.saturating_sub(*at) <= DEDUP_TTL_MS => {
                Some((*status, body.clone(), *elapsed))
            }
            _ => None,
        }
    }

    fn put(&mut self, key: String, status: u16, body: Vec<u8>, elapsed_ms: u64, now_ms: u64) {
        while self.order.len() >= DEDUP_CAP {
            if let Some(evicted) = self.order.pop_front() {
                self.map.remove(&evicted);
            }
        }
        self.map
            .insert(key.clone(), (status, body, elapsed_ms, now_ms));
        self.order.push_back(key);
    }
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn request_id_from(uri: &Uri) -> Option<String> {
    uri.query()?
        .split('&')
        .find_map(|kv| kv.strip_prefix("requestId="))
        .map(String::from)
}

pub struct ProxyState {
    client: reqwest::Client,
    upstream_base: String,
    bus: PluginBus,
    cache: Option<Arc<CachePlugin>>,
    coalesce: crate::config::CoalesceConfig,
    pub metrics: Arc<crate::metrics::Metrics>,
    breaker: crate::circuit_breaker::CircuitBreaker,
    /// In-flight identical reads: key → broadcast to followers. An entry
    /// exists only while the leader's upstream request is outstanding.
    inflight: Mutex<HashMap<[u8; 32], tokio::sync::broadcast::Sender<CoalescedResponse>>>,
    /// Session token → facts captured at login (never the credentials).
    sessions: Mutex<HashMap<String, Session>>,
    /// Completed read-only responses keyed by (token, requestId) for
    /// driver-retry replay.
    recent: Mutex<RecentResponses>,
    /// Realized-savings ledger; None when savings.enabled = false.
    ledger: Option<Arc<crate::savings::Ledger>>,
    /// Suspend plugin handle for the background sweeper.
    suspend: Option<Arc<crate::suspend::SuspendPlugin>>,
    /// chukei's own session for enforce-mode actions.
    service: Option<Arc<service::ServiceSession>>,
    suspend_config: crate::config::SuspendConfig,
}

impl ProxyState {
    pub fn from_config(config: &Config) -> Result<Arc<Self>> {
        let upstream = config.upstream.snowflake.as_ref().ok_or_else(|| {
            Error::Config("upstream.snowflake is required for `chukei up`".into())
        })?;
        let bundle = build_bus(config);
        let ledger = if config.savings.enabled {
            let pricing = crate::savings::Pricing::from_config(&config.savings);
            let path = config
                .savings
                .db_path
                .clone()
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::env::temp_dir().join("chukei-savings.db"));
            Some(Arc::new(crate::savings::Ledger::open(&path, pricing)?))
        } else {
            None
        };
        Ok(Arc::new(Self {
            ledger,
            suspend: bundle.suspend,
            service: service::ServiceSession::from_config(config)?.map(Arc::new),
            suspend_config: config.plugins.suspend.clone(),
            client: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(10))
                // Sync query-request blocks up to Snowflake's ~45 s ping
                // cycle; generous ceiling so we never cut a long DDL.
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .map_err(|e| Error::Connectivity(e.to_string()))?,
            upstream_base: upstream.base_url().trim_end_matches('/').to_string(),
            bus: bundle.bus,
            cache: bundle.cache,
            coalesce: config.coalesce.clone(),
            metrics: Arc::new(crate::metrics::Metrics::new()),
            breaker: crate::circuit_breaker::CircuitBreaker::default(),
            inflight: Mutex::new(HashMap::new()),
            sessions: Mutex::new(HashMap::new()),
            recent: Mutex::new(RecentResponses::default()),
        }))
    }

    fn session_for(&self, headers: &HeaderMap) -> Session {
        token_from_headers(headers)
            .and_then(|t| self.sessions.lock().unwrap().get(&t).cloned())
            .unwrap_or_default()
    }

    /// Test hook: the session facts currently associated with a token.
    pub fn session_user_for_token(&self, token: &str) -> Option<String> {
        self.sessions
            .lock()
            .unwrap()
            .get(token)
            .and_then(|s| s.user.clone())
    }

    /// Handle to the suspend plugin (None when disabled).
    pub fn suspend_plugin(&self) -> Option<Arc<crate::suspend::SuspendPlugin>> {
        self.suspend.clone()
    }

    /// Price + persist one avoided execution; non-blocking on the hot path.
    fn record_saving(
        self: &Arc<Self>,
        kind: &str,
        fingerprint: Option<[u8; 32]>,
        team: Option<String>,
        warehouse: Option<String>,
        canonical_elapsed_ms: u64,
    ) {
        let Some(ledger) = self.ledger.clone() else {
            return;
        };
        let (avoided_credits, avoided_usd) = ledger
            .pricing
            .avoided(warehouse.as_deref(), canonical_elapsed_ms);
        self.metrics.saved_usd_total.inc_by(avoided_usd);
        let event = crate::savings::SavingsEvent {
            at_unix_ms: unix_ms(),
            kind: kind.to_string(),
            fingerprint_hex: fingerprint.map(|f| crate::sql::fingerprint::hex(&f[..8])),
            team,
            warehouse,
            avoided_credits,
            avoided_usd,
            canonical_elapsed_ms,
        };
        tokio::task::spawn_blocking(move || {
            if let Err(e) = ledger.record(&event) {
                tracing::warn!(error = %e, "savings ledger write failed");
            }
        });
    }
}

pub fn router(state: Arc<ProxyState>) -> Router {
    Router::new()
        .route("/session/v1/login-request", post(login_request))
        .route("/session/token-request", post(token_request))
        .route("/queries/v1/query-request", post(query_request))
        .route("/chukei/healthz", axum::routing::get(|| async { "ok" }))
        .fallback(passthrough)
        .with_state(state)
}

/// Standalone observability listener (`/metrics`, `/healthz`) on the
/// configured Prometheus port — kept off the proxy port so scrapers never
/// need the customer TLS cert.
pub fn observability_router(metrics: Arc<crate::metrics::Metrics>) -> Router {
    Router::new()
        .route(
            "/metrics",
            axum::routing::get(move || {
                let metrics = metrics.clone();
                async move {
                    (
                        [(
                            hyper::header::CONTENT_TYPE,
                            "application/openmetrics-text; version=1.0.0; charset=utf-8",
                        )],
                        metrics.render(),
                    )
                }
            }),
        )
        .route("/healthz", axum::routing::get(|| async { "ok" }))
}

/// Run the proxy until the listener fails. When `listen.tls` is configured,
/// terminate TLS at chukei with the customer-owned cert (PRD §8.2 — the
/// Greybeam pattern: drivers point at `chukei.internal.<company>.com`).
pub async fn serve(config: &Config) -> Result<()> {
    serve_with_handle(config, axum_server::Handle::new()).await
}

/// Like [`serve`], but with an [`axum_server::Handle`] for shutdown control
/// and bound-address discovery (`handle.listening().await`).
pub async fn serve_with_handle(
    config: &Config,
    handle: axum_server::Handle<std::net::SocketAddr>,
) -> Result<()> {
    let state = ProxyState::from_config(config)?;
    let addr: std::net::SocketAddr =
        config.listen.bind.parse().map_err(|e| {
            Error::Config(format!("invalid listen.bind '{}': {e}", config.listen.bind))
        })?;

    spawn_sweeper(state.clone());

    // Drain in-flight queries on SIGTERM/ctrl-c (PRD §17: <5 s drain).
    let shutdown_handle = handle.clone();
    tokio::spawn(async move {
        wait_for_shutdown_signal().await;
        tracing::info!("shutdown signal received; draining in-flight queries (5s budget)");
        shutdown_handle.graceful_shutdown(Some(std::time::Duration::from_secs(5)));
    });

    if config.observability.prometheus.enabled {
        let obs_addr: std::net::SocketAddr =
            format!("0.0.0.0:{}", config.observability.prometheus.port)
                .parse()
                .map_err(|e| Error::Config(format!("invalid prometheus port: {e}")))?;
        let obs_router = observability_router(state.metrics.clone());
        tokio::spawn(async move {
            match tokio::net::TcpListener::bind(obs_addr).await {
                Ok(listener) => {
                    tracing::info!(bind = %obs_addr, "observability listener (/metrics, /healthz)");
                    if let Err(e) = axum::serve(listener, obs_router).await {
                        tracing::error!(error = %e, "observability listener failed");
                    }
                }
                Err(e) => {
                    tracing::error!(error = %e, bind = %obs_addr, "cannot bind observability port")
                }
            }
        });
    }

    match &config.listen.tls {
        Some(tls) => {
            // rustls 0.23 needs a process-default crypto provider; with
            // both ring (reqwest) and aws-lc-rs (axum-server) in the
            // dependency graph nothing is installed implicitly, and the
            // failure mode is a panic deep in the acceptor. Idempotent.
            let _ = rustls::crypto::ring::default_provider().install_default();
            let rustls_config =
                axum_server::tls_rustls::RustlsConfig::from_pem_file(&tls.cert, &tls.key)
                    .await
                    .map_err(|e| {
                        Error::Config(format!(
                            "cannot load TLS cert/key ({} / {}): {e}",
                            tls.cert, tls.key
                        ))
                    })?;
            tracing::info!(bind = %addr, upstream = %state.upstream_base, tls = true, "chukeid listening");
            axum_server::bind_rustls(addr, rustls_config)
                .handle(handle)
                .serve(router(state).into_make_service())
                .await
                .map_err(|e| Error::Connectivity(e.to_string()))
        }
        None => {
            tracing::warn!(
                "listen.tls not configured — serving plain HTTP; Snowflake drivers require TLS \
                 in production"
            );
            tracing::info!(bind = %addr, upstream = %state.upstream_base, tls = false, "chukeid listening");
            axum_server::bind(addr)
                .handle(handle)
                .serve(router(state).into_make_service())
                .await
                .map_err(|e| Error::Connectivity(e.to_string()))
        }
    }
}

/// Background sweeper: evaluates every observed warehouse against the
/// suspend model. `suggest-only` logs + counts; `enforce` executes
/// `ALTER WAREHOUSE … SUSPEND` through the service session and books the
/// avoided idle into the savings ledger. Per-warehouse cooldown prevents
/// repeat actions while a recommendation stays valid.
pub fn spawn_sweeper(state: Arc<ProxyState>) {
    let Some(suspend) = state.suspend.clone() else {
        return;
    };
    let interval = std::time::Duration::from_secs(state.suspend_config.sweep_interval_secs.max(1));
    let enforce = state.suspend_config.mode == crate::config::SuspendMode::Enforce;
    tokio::spawn(async move {
        let mut cooldown: HashMap<String, u64> = HashMap::new();
        const COOLDOWN_MS: u64 = 5 * 60 * 1000;
        loop {
            tokio::time::sleep(interval).await;
            let now = unix_ms();
            for warehouse in suspend.model.warehouses() {
                if cooldown
                    .get(&warehouse)
                    .is_some_and(|t| now.saturating_sub(*t) < COOLDOWN_MS)
                {
                    continue;
                }
                let Some(rec) = suspend.model.recommend(&warehouse, now) else {
                    continue;
                };
                state.metrics.suspend_recommendations_total.inc();
                if !enforce {
                    tracing::info!(
                        warehouse = %rec.warehouse,
                        p_arrival = rec.p_arrival_in_horizon,
                        expected_idle_secs_saved = rec.expected_idle_secs_saved,
                        "suspend recommendation (suggest-only; enable enforce to act)"
                    );
                    cooldown.insert(warehouse, now);
                    continue;
                }
                let Some(service) = state.service.clone() else {
                    tracing::error!("enforce mode without a service session; cannot suspend");
                    continue;
                };
                let sql = format!("ALTER WAREHOUSE \"{}\" SUSPEND", rec.warehouse);
                match service.execute(&sql).await {
                    Ok(resp) if resp.get("success").and_then(|s| s.as_bool()) == Some(true) => {
                        state.metrics.suspends_executed_total.inc();
                        state.record_saving(
                            "suspend",
                            None,
                            None,
                            Some(rec.warehouse.clone()),
                            (rec.expected_idle_secs_saved * 1000.0) as u64,
                        );
                        tracing::info!(warehouse = %rec.warehouse, "warehouse suspended");
                        cooldown.insert(warehouse, now);
                    }
                    Ok(resp) => {
                        tracing::warn!(
                            warehouse = %rec.warehouse,
                            message = resp.get("message").and_then(|m| m.as_str()).unwrap_or("?"),
                            "suspend statement rejected"
                        );
                        cooldown.insert(warehouse, now);
                    }
                    Err(e) => {
                        tracing::warn!(warehouse = %rec.warehouse, error = %e, "suspend failed");
                    }
                }
            }
        }
    });
}

async fn wait_for_shutdown_signal() {
    #[cfg(unix)]
    {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = sigterm.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

/// Official drivers gzip every non-trivial POST body
/// (`Content-Encoding: gzip` — snowflake-connector-python does this
/// unconditionally). To inspect a body we must decode it; to forward a
/// *modified* body we must re-encode it the same way, or the upstream
/// misparses it.
fn is_gzip(headers: &HeaderMap) -> bool {
    headers
        .get(hyper::header::CONTENT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("gzip"))
}

/// Decode the request body for inspection. Returns None when the body is
/// declared gzip but doesn't decode — the caller must fall back to verbatim
/// passthrough (F-001), never guess.
fn decode_body(headers: &HeaderMap, bytes: &[u8]) -> Option<Vec<u8>> {
    if !is_gzip(headers) {
        return Some(bytes.to_vec());
    }
    let mut decoder = flate2::read::GzDecoder::new(bytes);
    let mut out = Vec::new();
    std::io::Read::read_to_end(&mut decoder, &mut out).ok()?;
    Some(out)
}

/// Re-encode a modified body to match the client's original encoding.
fn encode_body_like(headers: &HeaderMap, bytes: Vec<u8>) -> Vec<u8> {
    if !is_gzip(headers) {
        return bytes;
    }
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    if std::io::Write::write_all(&mut encoder, &bytes).is_err() {
        return bytes;
    }
    encoder.finish().unwrap_or(bytes)
}

/// `Authorization: Snowflake Token="<token>"` (drivers) or a bare bearer.
fn token_from_headers(headers: &HeaderMap) -> Option<String> {
    let auth = headers.get(hyper::header::AUTHORIZATION)?.to_str().ok()?;
    if let Some(rest) = auth.strip_prefix("Snowflake Token=") {
        return Some(rest.trim_matches('"').to_string());
    }
    auth.strip_prefix("Bearer ").map(String::from)
}

fn is_hop_by_hop(name: &str) -> bool {
    matches!(
        name,
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailers"
            | "transfer-encoding"
            | "upgrade"
            | "host"
            | "content-length"
            | "accept-encoding"
    )
}

async fn forward(
    state: &ProxyState,
    method: Method,
    uri: &Uri,
    headers: &HeaderMap,
    body: Vec<u8>,
) -> Result<(StatusCode, HeaderMap, Vec<u8>)> {
    if !state.breaker.allow() {
        state.metrics.circuit_breaker_fast_fails_total.inc();
        return Err(Error::Connectivity(
            "upstream circuit breaker is open".into(),
        ));
    }

    let path_and_query = uri.path_and_query().map(|p| p.as_str()).unwrap_or("/");
    let url = format!("{}{}", state.upstream_base, path_and_query);

    let mut out_headers = reqwest::header::HeaderMap::new();
    for (name, value) in headers {
        if !is_hop_by_hop(name.as_str()) {
            out_headers.insert(name.clone(), value.clone());
        }
    }

    let response = state
        .client
        .request(method, &url)
        .headers(out_headers)
        .body(body)
        .send()
        .await
        .map_err(|e| {
            state.breaker.record_failure();
            state.metrics.upstream_errors_total.inc();
            Error::Connectivity(format!("upstream request failed: {e}"))
        })?;
    state.breaker.record_success();

    let status = response.status();
    let mut resp_headers = HeaderMap::new();
    for (name, value) in response.headers() {
        if !is_hop_by_hop(name.as_str()) {
            resp_headers.insert(name.clone(), value.clone());
        }
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|e| Error::Connectivity(format!("upstream body read failed: {e}")))?;
    Ok((status, resp_headers, bytes.to_vec()))
}

fn respond(status: StatusCode, headers: HeaderMap, body: Vec<u8>) -> Response {
    let mut response = Response::builder().status(status);
    if let Some(h) = response.headers_mut() {
        *h = headers;
    }
    response.body(Body::from(body)).unwrap_or_else(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "chukei: response build failed",
        )
            .into_response()
    })
}

fn upstream_unavailable(err: Error) -> Response {
    // PRD §8.5: clients treat 503 as transient.
    tracing::error!(error = %err, "upstream unreachable");
    (StatusCode::SERVICE_UNAVAILABLE, format!("chukei: {err}")).into_response()
}

/// Generic passthrough for every endpoint we don't inspect.
async fn passthrough(State(state): State<Arc<ProxyState>>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let bytes = match to_bytes(body, MAX_INSPECT_BYTES).await {
        Ok(b) => b.to_vec(),
        Err(_) => return (StatusCode::PAYLOAD_TOO_LARGE, "chukei: body too large").into_response(),
    };
    match forward(&state, parts.method, &parts.uri, &parts.headers, bytes).await {
        Ok((status, headers, body)) => respond(status, headers, body),
        Err(e) => upstream_unavailable(e),
    }
}

/// Login: pass through, then capture (token → session facts) from the
/// request's CLIENT_APP_ID / LOGIN_NAME and the response's token. Tokens are
/// held in memory only, never logged, never persisted.
async fn login_request(State(state): State<Arc<ProxyState>>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let bytes = match to_bytes(body, MAX_INSPECT_BYTES).await {
        Ok(b) => b.to_vec(),
        Err(_) => return (StatusCode::PAYLOAD_TOO_LARGE, "chukei: body too large").into_response(),
    };

    let login: Option<serde_json::Value> = decode_body(&parts.headers, &bytes)
        .and_then(|decoded| serde_json::from_slice(&decoded).ok());
    let (status, headers, resp_body) =
        match forward(&state, parts.method, &parts.uri, &parts.headers, bytes).await {
            Ok(r) => r,
            Err(e) => return upstream_unavailable(e),
        };

    if let Ok(resp) = serde_json::from_slice::<serde_json::Value>(&resp_body) {
        if let Some(token) = resp.pointer("/data/token").and_then(|t| t.as_str()) {
            let data = login.as_ref().and_then(|l| l.get("data"));
            // Warehouse: the login response's sessionInfo is authoritative;
            // fall back to the request's ?warehouse= query param. Without
            // this the suspend model and per-warehouse savings pricing never
            // see a warehouse name at all (shipped blind in ≤0.2.0).
            let warehouse = resp
                .pointer("/data/sessionInfo/warehouseName")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from)
                .or_else(|| query_param(&parts.uri, "warehouse"));
            let session = Session {
                user: data
                    .and_then(|d| d.get("LOGIN_NAME"))
                    .and_then(|v| v.as_str())
                    .map(String::from),
                application_name: data
                    .and_then(|d| d.get("CLIENT_APP_ID"))
                    .and_then(|v| v.as_str())
                    .map(String::from),
                warehouse,
                ..Default::default()
            };
            state
                .sessions
                .lock()
                .unwrap()
                .insert(token.to_string(), session);
        }
    }
    respond(status, headers, resp_body)
}

/// Extract a query-string parameter from a request URI.
fn query_param(uri: &axum::http::Uri, name: &str) -> Option<String> {
    uri.query()?.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k.eq_ignore_ascii_case(name) && !v.is_empty()).then(|| v.to_string())
    })
}

/// Token renewal: drivers auto-renew on session expiry (error 390112) via
/// this endpoint, after which the `Authorization` token changes. Re-key the
/// session-facts map so attribution survives renewal.
async fn token_request(State(state): State<Arc<ProxyState>>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let bytes = match to_bytes(body, MAX_INSPECT_BYTES).await {
        Ok(b) => b.to_vec(),
        Err(_) => return (StatusCode::PAYLOAD_TOO_LARGE, "chukei: body too large").into_response(),
    };
    let old_token = decode_body(&parts.headers, &bytes)
        .and_then(|d| serde_json::from_slice::<serde_json::Value>(&d).ok())
        .and_then(|v| {
            v.get("oldSessionToken")
                .or_else(|| v.pointer("/data/oldSessionToken"))
                .and_then(|t| t.as_str())
                .map(String::from)
        });

    let (status, headers, resp_body) =
        match forward(&state, parts.method, &parts.uri, &parts.headers, bytes).await {
            Ok(r) => r,
            Err(e) => return upstream_unavailable(e),
        };

    if let (Some(old), Ok(resp)) = (
        old_token,
        serde_json::from_slice::<serde_json::Value>(&resp_body),
    ) {
        let new_token = resp
            .pointer("/data/sessionToken")
            .or_else(|| resp.pointer("/data/token"))
            .and_then(|t| t.as_str());
        if let Some(new) = new_token {
            let mut sessions = state.sessions.lock().unwrap();
            if let Some(session) = sessions.remove(&old) {
                sessions.insert(new.to_string(), session);
            }
        }
    }
    respond(status, headers, resp_body)
}

/// Blame mode (PRD §11.1): re-run a sampled cache hit against upstream and
/// assert the data matches. A mismatch is the worst failure chukei can have;
/// it alerts and evicts the entry.
async fn blame_check(
    state: Arc<ProxyState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Vec<u8>,
    expected: serde_json::Value,
    fingerprint: [u8; 32],
) {
    let Ok((status, _headers, resp_body)) = forward(&state, method, &uri, &headers, body).await
    else {
        return; // upstream unreachable: nothing to compare
    };
    if status != StatusCode::OK {
        return;
    }
    let Ok(actual) = serde_json::from_slice::<serde_json::Value>(&resp_body) else {
        return;
    };
    if actual.get("success").and_then(|s| s.as_bool()) != Some(true) {
        return;
    }
    let has_chunks = actual
        .pointer("/data/chunks")
        .and_then(|c| c.as_array())
        .is_some_and(|c| !c.is_empty());
    if has_chunks {
        return; // chunked responses aren't comparable byte-for-byte
    }
    let same = actual.pointer("/data/rowset") == expected.pointer("/data/rowset")
        && actual.pointer("/data/rowsetBase64") == expected.pointer("/data/rowsetBase64");
    if !same {
        state.metrics.cache_blame_mismatches_total.inc();
        tracing::error!(
            fingerprint = %crate::sql::fingerprint::hex(&fingerprint[..8]),
            "BLAME MISMATCH: cached result differs from upstream; evicting entry"
        );
        if let Some(cache) = &state.cache {
            cache.remove(&fingerprint);
        }
    }
}

/// Key for in-flight coalescing, or None when the query must not coalesce.
fn coalesce_key(
    state: &ProxyState,
    analysis: &crate::sql::QueryAnalysis,
    payload: Option<&serde_json::Value>,
    headers: &HeaderMap,
) -> Option<[u8; 32]> {
    if !state.coalesce.enabled
        || !crate::sql::parse::is_read_only(&analysis.statement)
        || !analysis.features.deterministic
    {
        return None;
    }
    let mut hasher = blake3::Hasher::new();
    hasher.update(&analysis.hard_fingerprint);
    // Bind variables change results even when the SQL text is identical.
    let bindings = payload
        .and_then(|p| p.get("bindings"))
        .map(|b| b.to_string())
        .unwrap_or_default();
    hasher.update(bindings.as_bytes());
    if state.coalesce.scope == crate::config::CoalesceScope::Session {
        hasher.update(token_from_headers(headers).unwrap_or_default().as_bytes());
    }
    Some(*hasher.finalize().as_bytes())
}

/// The one inspected endpoint. Failure of anything chukei-side falls back to
/// verbatim passthrough — the proxy must never break a query it can't help.
async fn query_request(State(state): State<Arc<ProxyState>>, request: Request) -> Response {
    let started = Instant::now();
    let (parts, body) = request.into_parts();
    let bytes = match to_bytes(body, MAX_INSPECT_BYTES).await {
        Ok(b) => b.to_vec(),
        Err(_) => return (StatusCode::PAYLOAD_TOO_LARGE, "chukei: body too large").into_response(),
    };

    let mut payload: Option<serde_json::Value> = decode_body(&parts.headers, &bytes)
        .and_then(|decoded| serde_json::from_slice(&decoded).ok());
    let sql = payload
        .as_ref()
        .and_then(|p| p.get("sqlText"))
        .and_then(|s| s.as_str())
        .map(String::from);

    let analysis = sql.as_deref().and_then(|s| analyze(s).ok());

    let Some(analysis) = analysis else {
        // Unparseable or non-JSON: forward untouched (F-001).
        let result = forward(&state, parts.method, &parts.uri, &parts.headers, bytes).await;
        return match result {
            Ok((status, headers, body)) => respond(status, headers, body),
            Err(e) => upstream_unavailable(e),
        };
    };

    let read_only = crate::sql::parse::is_read_only(&analysis.statement);
    let request_token = token_from_headers(&parts.headers).unwrap_or_default();
    // The key covers token + requestId + the exact query identity: requestId
    // alone is only trustworthy for retries of the SAME logical request — a
    // client reusing an id with different SQL must never get a stale replay.
    let dedup_key = request_id_from(&parts.uri)
        .filter(|_| read_only && analysis.features.deterministic)
        .map(|rid| {
            let bindings = payload
                .as_ref()
                .and_then(|p| p.get("bindings"))
                .map(|b| b.to_string())
                .unwrap_or_default();
            format!(
                "{request_token}:{rid}:{}:{}",
                crate::sql::fingerprint::hex(&analysis.hard_fingerprint[..8]),
                blake3::hash(bindings.as_bytes()).to_hex()
            )
        });

    // Driver retry replay: same (token, requestId) within the window means
    // the driver re-sent a request whose response it lost — replay it.
    if let Some(key) = &dedup_key {
        let replay = state.recent.lock().unwrap().get(key, unix_ms());
        if let Some((status, body, canonical_elapsed_ms)) = replay {
            state.metrics.route("retry_replay");
            let warehouse = state.session_for(&parts.headers).warehouse;
            state.record_saving(
                "retry_replay",
                Some(analysis.hard_fingerprint),
                None,
                warehouse,
                canonical_elapsed_ms,
            );
            tracing::info!(request_id = %key, "replaying completed response for driver retry");
            let mut headers = HeaderMap::new();
            headers.insert(
                hyper::header::CONTENT_TYPE,
                hyper::header::HeaderValue::from_static("application/json"),
            );
            return respond(
                StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                headers,
                body,
            );
        }
    }

    // USE WAREHOUSE switches the session's warehouse mid-stream; track it so
    // suspend modelling and savings pricing follow the client.
    if let sqlparser::ast::Statement::Use(sqlparser::ast::Use::Warehouse(name)) =
        &analysis.statement
    {
        if let Some(token) = token_from_headers(&parts.headers) {
            if let Some(session) = state.sessions.lock().unwrap().get_mut(&token) {
                session.warehouse = Some(name.to_string().replace('"', ""));
            }
        }
    }

    let session = state.session_for(&parts.headers);
    let ctx = QueryContext {
        analysis: &analysis,
        session: &session,
    };
    let resolution = state.bus.decide(&ctx).await;

    if let Some(veto) = &resolution.veto {
        state.metrics.route("veto");
        let body = serde_json::json!({
            "success": false,
            "message": format!("chukei policy veto: {}", veto.0),
            "code": "390114",
        });
        return respond(
            StatusCode::OK,
            HeaderMap::new(),
            body.to_string().into_bytes(),
        );
    }

    let cache_eligible = state.cache.is_some()
        && read_only
        && analysis.features.deterministic
        && !analysis.features.tables.is_empty();

    if let (Some(key), Some(cache)) = (&resolution.serve_from_cache, &state.cache) {
        if let Some(entry) = cache.lookup(&key.hard_fingerprint) {
            state.metrics.cache_hits_total.inc();
            state.metrics.route("cache_hit");
            state.record_saving(
                "cache_hit",
                Some(key.hard_fingerprint),
                resolution.annotations.get("chukei.team").cloned(),
                session.warehouse.clone(),
                entry.canonical_wall_clock_ms,
            );
            tracing::info!(
                fingerprint = %crate::sql::fingerprint::hex(&key.hard_fingerprint[..8]),
                overhead_ms = started.elapsed().as_millis() as u64,
                "cache hit"
            );
            // Blame mode: sample-rerun this hit upstream in the background.
            if rand::random::<f64>() < cache.blame_sample_rate() {
                tokio::spawn(blame_check(
                    state.clone(),
                    parts.method.clone(),
                    parts.uri.clone(),
                    parts.headers.clone(),
                    bytes.clone(),
                    entry.data.clone(),
                    key.hard_fingerprint,
                ));
            }
            return respond(
                StatusCode::OK,
                HeaderMap::new(),
                entry.data.to_string().into_bytes(),
            );
        }
    }
    if cache_eligible {
        state.metrics.cache_misses_total.inc();
    }

    if let Some(engine) = &resolution.route {
        // DuckDB/Trino execution arrives with the engine abstraction; until
        // then, routing decisions are observed but not acted on.
        tracing::info!(
            ?engine,
            "router selected alternate engine; engine not linked, passing through"
        );
    }

    state.metrics.route(if resolution.rewrite.is_some() {
        "rewritten"
    } else {
        "passthrough"
    });
    state
        .metrics
        .proxy_overhead_seconds
        .observe(started.elapsed().as_secs_f64());

    let outbound = if let Some(rewritten) = &resolution.rewrite {
        state.metrics.rewrites_total.inc();
        tracing::info!(rules = ?resolution.annotations.get("chukei.rewrite_rules"), "rewrite applied");
        match payload.as_mut() {
            Some(p) => {
                p["sqlText"] = serde_json::Value::String(rewritten.clone());
                match serde_json::to_vec(p) {
                    // Match the client's Content-Encoding or upstream misparses.
                    Ok(json) => encode_body_like(&parts.headers, json),
                    Err(_) => bytes,
                }
            }
            None => bytes,
        }
    } else {
        bytes
    };

    tracing::debug!(
        overhead_us = started.elapsed().as_micros() as u64,
        "hot-path overhead"
    );

    // ── in-flight coalescing ─────────────────────────────────────────────
    // Identical deterministic reads that are concurrently outstanding share
    // one upstream execution (dashboard-refresh storms). The key covers the
    // hard fingerprint (canonical SQL + literals), the bind variables, and —
    // under the default `session` scope — the session token, so row-level
    // security can never leak rows across sessions.
    let coalesce_key = coalesce_key(&state, &analysis, payload.as_ref(), &parts.headers);
    if let Some(key) = &coalesce_key {
        loop {
            // Scope the lock so the guard never lives across an await.
            let follower_rx = {
                let mut inflight = state.inflight.lock().unwrap();
                match inflight.get(key) {
                    Some(tx) => Some(tx.subscribe()),
                    None => {
                        let (tx, _) = tokio::sync::broadcast::channel(1);
                        inflight.insert(*key, tx);
                        None
                    }
                }
            };
            let Some(mut rx) = follower_rx else {
                break; // we are the leader
            };
            match rx.recv().await {
                Ok(shared) => {
                    state.metrics.coalesced_total.inc();
                    state.metrics.route("coalesced");
                    state.record_saving(
                        "coalesced",
                        Some(analysis.hard_fingerprint),
                        resolution.annotations.get("chukei.team").cloned(),
                        session.warehouse.clone(),
                        shared.2,
                    );
                    tracing::info!(
                        fingerprint = %crate::sql::fingerprint::hex(&key[..8]),
                        "coalesced onto in-flight identical query"
                    );
                    let status =
                        StatusCode::from_u16(shared.0).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                    let mut headers = HeaderMap::new();
                    headers.insert(
                        hyper::header::CONTENT_TYPE,
                        hyper::header::HeaderValue::from_static("application/json"),
                    );
                    return respond(status, headers, shared.1.clone());
                }
                // Leader failed before broadcasting; retake the map.
                Err(_) => continue,
            }
        }
    }

    let upstream_started = Instant::now();
    let forwarded = forward(&state, parts.method, &parts.uri, &parts.headers, outbound).await;
    let upstream_elapsed_ms = upstream_started.elapsed().as_millis() as u64;

    // Leader always clears the entry; followers either get the broadcast or
    // observe the closed channel and re-enter the loop independently.
    let coalesce_tx = coalesce_key
        .as_ref()
        .and_then(|key| state.inflight.lock().unwrap().remove(key));

    let (status, headers, resp_body) = match forwarded {
        Ok(r) => r,
        Err(e) => return upstream_unavailable(e),
    };

    if let Some(tx) = coalesce_tx {
        let _ = tx.send(Arc::new((
            status.as_u16(),
            resp_body.clone(),
            upstream_elapsed_ms,
        )));
    }

    // Lineage invalidation for writes observed through the proxy: a write
    // to table X tombstones every cache entry that read X. (Out-of-band
    // writes still need the QUERY_HISTORY watcher — documented gap.)
    if !read_only && status == StatusCode::OK {
        if let Some(cache) = &state.cache {
            for table in &analysis.features.tables {
                let removed = cache.invalidate_table(table);
                if removed > 0 {
                    tracing::info!(table = %table, removed, "cache invalidated by observed write");
                }
            }
        }
    }

    // Remember the completed response for driver-retry replay.
    if let Some(key) = dedup_key {
        state.recent.lock().unwrap().put(
            key,
            status.as_u16(),
            resp_body.clone(),
            upstream_elapsed_ms,
            unix_ms(),
        );
    }

    // Feed the result back to plugins (cache fill, model updates). Only
    // small, chunkless, successful JSON responses are cacheable verbatim —
    // chunked Arrow downloads carry expiring URLs (PRD §8.1).
    if status == StatusCode::OK {
        if let Ok(resp) = serde_json::from_slice::<serde_json::Value>(&resp_body) {
            let success = resp
                .get("success")
                .and_then(|s| s.as_bool())
                .unwrap_or(false);
            let has_chunks = resp
                .pointer("/data/chunks")
                .and_then(|c| c.as_array())
                .is_some_and(|c| !c.is_empty());
            let snapshot = ResultSnapshot {
                served_from_cache: false,
                wall_clock_ms: upstream_elapsed_ms,
                data: (success && !has_chunks).then(|| resp.clone()),
                ..Default::default()
            };
            state.bus.on_result(&ctx, &snapshot).await;
        }
    }

    respond(status, headers, resp_body)
}
