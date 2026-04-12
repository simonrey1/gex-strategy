use axum::{extract::Query, http::StatusCode, response::Json, routing::get, Router};
use std::collections::HashMap;
use tower_http::services::ServeDir;

use crate::data::paths::{data_dir, list_backtest_tickers, project_root, read_backtest_json};

// ─── Handlers ────────────────────────────────────────────────────────────────

async fn handle_tickers() -> Json<serde_json::Value> {
    Json(serde_json::to_value(list_backtest_tickers()).unwrap_or_default())
}

async fn handle_backtest(
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    read_backtest_json(params.get("ticker").map(|s| s.as_str()))
        .map(Json)
        .map_err(|e| match e {
            "not_found" => StatusCode::NOT_FOUND,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        })
}

async fn handle_missed_entries() -> Result<Json<serde_json::Value>, StatusCode> {
    let path = data_dir().join("results").join("missed-entries.json");
    let contents = std::fs::read_to_string(&path).map_err(|_| StatusCode::NOT_FOUND)?;
    let val: serde_json::Value = serde_json::from_str(&contents).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(val))
}

pub async fn serve_dashboard(port: u16, tickers: &[crate::config::tickers::Ticker]) {
    let dashboard_dir = project_root().join("dashboard").join("dist");
    if !dashboard_dir.join("index.html").exists() {
        println!("\n  [serve] Dashboard not built. Run: npm --prefix dashboard run build");
        return;
    }

    let app = Router::new()
        .route("/api/backtest/tickers", get(handle_tickers))
        .route("/api/backtest", get(handle_backtest))
        .route("/api/backtest/missed-entries", get(handle_missed_entries))
        .fallback_service(
            ServeDir::new(&dashboard_dir).append_index_html_on_directories(true),
        );

    let ticker_param = if tickers.len() == 1 {
        format!("&ticker={}", tickers[0].as_str())
    } else {
        String::new()
    };
    let url = format!("http://localhost:{}?tab=backtest{}", port, ticker_param);
    println!("\n  Dashboard -> {}", url);
    println!("  Press Ctrl+C to stop\n");

    let addr = format!("127.0.0.1:{}", port);
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[serve] Failed to bind :{} — {}", port, e);
            return;
        }
    };
    if let Err(e) = axum::serve(listener, app).await {
        eprintln!("[serve] Server error: {}", e);
    }
}
