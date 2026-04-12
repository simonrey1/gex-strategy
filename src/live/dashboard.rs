use axum::{
    extract::State,
    http::StatusCode,
    middleware,
    response::Json,
    routing::get,
    Router,
};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tower_http::services::ServeDir;
use ts_rs::TS;

use super::auth::{basic_auth_middleware, ServerConfig};

use crate::data::paths::project_root;
use crate::config::Ticker;
use crate::types::Signal;
use crate::data::thetadata_live::{get_gex_status, get_live_gex_profile, GexStreamStatus as LiveGexStreamStatus, SharedLiveGex};

pub use super::dashboard_types::{
    IbkrPositionRow, IbkrOrderRow, TickerHealth, TickerIndicators,
};

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "shared/generated/", rename = "LiveStatus")]
struct LiveStatusResponse {
    status: String,
    #[serde(rename = "brokerConnected")]
    broker_connected: bool,
    #[serde(rename = "upSince")]
    up_since: String,
    #[serde(rename = "uptimeSeconds")]
    uptime_seconds: u64,
    tickers: Vec<TickerStatusResponse>,
    #[serde(rename = "gexStream")]
    gex_stream: Option<LiveGexStreamStatus>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "shared/generated/", rename = "LiveTickerStatus")]
struct TickerStatusResponse {
    ticker: String,
    #[serde(rename = "lastPollMs")]
    last_poll_ms: u64,
    #[serde(rename = "lastPollAgoSeconds")]
    last_poll_ago_seconds: Option<u64>,
    #[serde(rename = "hasPosition")]
    has_position: bool,
    signal: Option<Signal>,
    #[serde(rename = "spotPrice")]
    spot_price: f64,
    #[serde(rename = "lastBarTime")]
    last_bar_time: Option<String>,
    #[serde(rename = "barsToday")]
    bars_today: u32,
    equity: f64,
    #[serde(rename = "consecutiveFailures")]
    consecutive_failures: u32,
    #[serde(rename = "lastError")]
    last_error: Option<String>,
    #[serde(rename = "putWall")]
    put_wall: Option<f64>,
    #[serde(rename = "callWall")]
    call_wall: Option<f64>,
    #[serde(rename = "netGex")]
    net_gex: Option<f64>,
    #[serde(rename = "lastGexMs")]
    last_gex_ms: Option<u64>,
    #[serde(rename = "lastGexAgoSeconds")]
    last_gex_ago_seconds: Option<u64>,
    indicators: Option<TickerIndicators>,
    #[serde(rename = "warmupStatus")]
    warmup_status: Option<String>,
}

pub struct HealthState {
    pub up_since: chrono::DateTime<chrono::Utc>,
    pub tickers: HashMap<String, TickerHealth>,
    pub broker_connected: bool,
    pub live_gex: Option<SharedLiveGex>,
    pub ibkr_client: Option<Arc<ibapi::Client>>,
}

pub type SharedHealthState = Arc<Mutex<HealthState>>;

pub fn new_health_state() -> SharedHealthState {
    Arc::new(Mutex::new(HealthState {
        up_since: chrono::Utc::now(),
        tickers: HashMap::new(),
        broker_connected: false,
        live_gex: None,
        ibkr_client: None,
    }))
}

pub fn set_health_gex(state: &SharedHealthState, gex: SharedLiveGex) {
    let mut s = super::lock_or_recover(state);
    s.live_gex = Some(gex);
}

pub fn set_health_ibkr(state: &SharedHealthState, client: Arc<ibapi::Client>) {
    let mut s = super::lock_or_recover(state);
    s.ibkr_client = Some(client);
}

pub fn prepopulate_tickers(state: &SharedHealthState, tickers: &[Ticker]) {
    let mut s = super::lock_or_recover(state);
    for &t in tickers {
        s.tickers.entry(t.as_str().to_string()).or_default();
    }
}

pub fn update_health(state: &SharedHealthState, ticker: &str, partial: TickerHealth) {
    let mut s = super::lock_or_recover(state);
    s.tickers.insert(ticker.to_string(), partial);
}

async fn handle_health() -> StatusCode {
    StatusCode::OK
}

async fn handle_status(
    State(state): State<SharedHealthState>,
) -> Json<serde_json::Value> {
    let s = super::lock_or_recover(&state);
    let now_ms = crate::types::now_ms();

    let live_gex_ref = s.live_gex.clone();
    let gex_poll_ms: Option<u64> = live_gex_ref.as_ref().map(|gex| get_gex_status(gex).last_poll_ms);

    let tickers: Vec<TickerStatusResponse> = s
        .tickers
        .iter()
        .map(|(ticker, th)| {
            let ticker_enum = Ticker::from_str_opt(ticker);

            let (put_wall, call_wall, net_gex) = live_gex_ref
                .as_ref()
                .and_then(|gex| {
                    let t = ticker_enum?;
                    let profile = get_live_gex_profile(gex, t)?;
                    let pw = profile.pw_opt();
                    let cw = profile.cw_opt();
                    let ng = if profile.net_gex.is_finite() { Some(profile.net_gex) } else { None };
                    Some((pw, cw, ng))
                })
                .unwrap_or((None, None, None));

            let last_gex_ms = gex_poll_ms.filter(|&ms| ms > 0);

            TickerStatusResponse {
                ticker: ticker.clone(),
                last_poll_ms: th.last_poll_ms,
                last_poll_ago_seconds: if th.last_poll_ms > 0 {
                    Some((now_ms - th.last_poll_ms) / 1000)
                } else {
                    None
                },
                has_position: th.position,
                signal: th.signal,
                spot_price: th.spot_price,
                last_bar_time: th.last_bar_time.clone(),
                bars_today: th.bars_today,
                equity: th.equity,
                consecutive_failures: th.consecutive_failures,
                last_error: th.last_error.clone(),
                put_wall,
                call_wall,
                net_gex,
                last_gex_ms,
                last_gex_ago_seconds: last_gex_ms.map(|ms| (now_ms - ms) / 1000),
                indicators: th.indicators.clone(),
                warmup_status: th.warmup_status.clone(),
            }
        })
        .collect();

    let uptime = (chrono::Utc::now() - s.up_since).num_seconds().unsigned_abs();

    let gex_stream = s.live_gex.as_ref().map(get_gex_status);

    let response = LiveStatusResponse {
        status: "ok".to_string(),
        broker_connected: s.broker_connected,
        up_since: s.up_since.to_rfc3339(),
        uptime_seconds: uptime,
        tickers,
        gex_stream,
    };

    Json(serde_json::to_value(response).unwrap_or_else(|e| {
        eprintln!("[health] status serialize failed: {:?}", e);
        serde_json::json!({"status": "error", "detail": "serialize_failed"})
    }))
}

async fn handle_trades() -> Json<serde_json::Value> {
    use super::trade_log::read_trade_log;
    let trades = read_trade_log();
    Json(serde_json::to_value(trades).unwrap_or(serde_json::json!([])))
}

async fn handle_backtest(
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    crate::data::paths::read_backtest_json(params.get("ticker").map(|s| s.as_str()))
        .map(Json)
        .map_err(|e| match e {
            "not_found" => StatusCode::NOT_FOUND,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        })
}

fn get_ibkr_client(state: &SharedHealthState) -> Option<Arc<ibapi::Client>> {
    super::lock_or_recover(state).ibkr_client.clone()
}

async fn handle_positions(
    State(state): State<SharedHealthState>,
) -> Json<serde_json::Value> {
    let Some(client) = get_ibkr_client(&state) else {
        return Json(serde_json::json!([]));
    };

    let rows = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let mut rows: Vec<IbkrPositionRow> = Vec::new();
        match client.positions().await {
            Ok(mut sub) => {
                while let Some(item) = sub.next().await {
                    match item {
                        Ok(ibapi::accounts::PositionUpdate::Position(p)) => {
                            if p.position.abs() > 0.001 {
                                rows.push(IbkrPositionRow {
                                    symbol: p.contract.symbol.to_string(),
                                    shares: p.position,
                                    avg_cost: p.average_cost,
                                });
                            }
                        }
                        Ok(ibapi::accounts::PositionUpdate::PositionEnd) => break,
                        Err(_) => break,
                    }
                }
            }
            Err(e) => {
                eprintln!("[health] positions() failed: {:?}", e);
            }
        }
        rows
    }).await.unwrap_or_default();

    Json(serde_json::to_value(&rows).unwrap_or(serde_json::json!([])))
}

async fn handle_orders(
    State(state): State<SharedHealthState>,
) -> Json<serde_json::Value> {
    let Some(client) = get_ibkr_client(&state) else {
        return Json(serde_json::json!([]));
    };

    let rows = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let mut rows: Vec<IbkrOrderRow> = Vec::new();
        match client.all_open_orders().await {
            Ok(mut sub) => {
                while let Some(item) = sub.next().await {
                    match item {
                        Ok(ibapi::orders::Orders::OrderData(od)) => {
                            rows.push(IbkrOrderRow {
                                order_id: od.order_id,
                                symbol: od.contract.symbol.to_string(),
                                action: format!("{:?}", od.order.action),
                                order_type: od.order.order_type.clone(),
                                quantity: od.order.total_quantity,
                                limit_price: od.order.limit_price,
                                stop_price: od.order.aux_price,
                                status: od.order_state.status.clone(),
                                filled: 0.0,
                                remaining: od.order.total_quantity,
                            });
                        }
                        Ok(ibapi::orders::Orders::OrderStatus(os)) => {
                            if let Some(row) = rows.iter_mut().find(|r| r.order_id == os.order_id) {
                                row.status = os.status.clone();
                                row.filled = os.filled;
                                row.remaining = os.remaining;
                            }
                        }
                        Ok(ibapi::orders::Orders::Notice(_)) => {}
                        Err(_) => break,
                    }
                }
            }
            Err(e) => {
                eprintln!("[health] all_open_orders() failed: {:?}", e);
            }
        }
        rows
    }).await.unwrap_or_default();

    Json(serde_json::to_value(&rows).unwrap_or(serde_json::json!([])))
}

async fn handle_backtest_tickers() -> Json<serde_json::Value> {
    Json(serde_json::to_value(crate::data::paths::list_backtest_tickers()).unwrap_or_default())
}

pub async fn start_health_server(server_cfg: ServerConfig, state: SharedHealthState) {
    let dashboard_dir = project_root().join("dashboard").join("dist");
    let has_dashboard = dashboard_dir.join("index.html").exists();
    let port = server_cfg.port;

    let protected = Router::new()
        .route("/api/status", get(handle_status))
        .route("/api/trades", get(handle_trades))
        .route("/api/positions", get(handle_positions))
        .route("/api/orders", get(handle_orders))
        .route("/api/backtest", get(handle_backtest))
        .route("/api/backtest/tickers", get(handle_backtest_tickers))
        .layer(middleware::from_fn(basic_auth_middleware));

    let mut app = Router::new()
        .route("/health", get(handle_health))
        .merge(protected)
        .with_state(state);

    if has_dashboard {
        app = app.fallback_service(ServeDir::new(&dashboard_dir).append_index_html_on_directories(true));
    }

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    let scheme = if server_cfg.has_tls() { "https" } else { "http" };
    println!(
        "[health] Server on {}://0.0.0.0:{} | dashboard={}",
        scheme, port,
        if has_dashboard { "yes" } else { "no (run: npm --prefix dashboard run build)" }
    );

    if let (Some(cert), Some(key)) = (&server_cfg.tls_cert, &server_cfg.tls_key) {
        let tls_config = match axum_server::tls_rustls::RustlsConfig::from_pem_file(cert, key).await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[health] TLS config failed: {} — falling back to HTTP", e);
                serve_plain(addr, app).await;
                return;
            }
        };
        if let Err(e) = axum_server::bind_rustls(addr, tls_config)
            .serve(app.into_make_service())
            .await
        {
            eprintln!("[health] TLS server error: {}", e);
        }
    } else {
        serve_plain(addr, app).await;
    }
}

async fn serve_plain(addr: std::net::SocketAddr, app: Router) {
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[health] Failed to bind :{} — {}", addr.port(), e);
            return;
        }
    };
    if let Err(e) = axum::serve(listener, app).await {
        eprintln!("[health] Server error: {}", e);
    }
}
