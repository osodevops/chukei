//! chukei-mock-sf: a minimal, fast mock of the Snowflake REST origin
//! (PRD §21.2) for integration and load testing.
//!
//! ```bash
//! cargo run --release --example mock_sf -- 127.0.0.1:18999
//! ```

use axum::routing::any;
use axum::{Json, Router};

#[tokio::main]
async fn main() {
    let bind = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:18999".to_string());
    let app = Router::new().fallback(any(|| async {
        Json(serde_json::json!({
            "success": true,
            "data": {
                "rowset": [["1", "alpha"], ["2", "beta"]],
                "rowtype": [{"name": "ID"}, {"name": "NAME"}],
                "total": 2,
                "queryId": "01b2c3d4-mock"
            }
        }))
    }));
    let listener = tokio::net::TcpListener::bind(&bind).await.expect("bind");
    eprintln!("mock-sf listening on {bind}");
    axum::serve(listener, app).await.unwrap();
}
