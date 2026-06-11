//! Reproducer for bug #1 (shipped in v0.1.0): any code path that builds a
//! rustls config *outside* `wire::sf::serve` — `chukei doctor`'s TLS probe
//! was the first casualty — panicked with "Could not automatically determine
//! the process-level CryptoProvider" because the provider was only installed
//! inside the serve path. The fix exposes `chukei_core::init_crypto()` and
//! calls it at CLI process start.
//!
//! This lives in its own integration-test binary on purpose: each file under
//! tests/ runs as a separate process, so no other test can have installed
//! the provider before this one runs.

#[tokio::test]
async fn tls_config_loads_without_serve_having_run() {
    chukei_core::init_crypto();

    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let cert_path = dir.path().join("t.crt");
    let key_path = dir.path().join("t.key");
    std::fs::write(&cert_path, cert.pem()).unwrap();
    std::fs::write(&key_path, signing_key.serialize_pem()).unwrap();

    // Pre-fix this panicked rather than returning an error.
    axum_server::tls_rustls::RustlsConfig::from_pem_file(&cert_path, &key_path)
        .await
        .expect("TLS config must load once init_crypto() has run");
}
