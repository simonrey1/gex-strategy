use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;

use crate::broker::ibkr::{IbkrBroker, OrderIdGen};
use crate::broker::types::Broker as _;
use crate::config::{StrategyConfig, Ticker};

use super::auth::ServerConfig;
use super::dashboard::{
    new_health_state, prepopulate_tickers, set_health_gex, set_health_ibkr,
    start_health_server, update_health, SharedHealthState, TickerHealth,
};
use super::orders::{new_fill_state, start_order_monitor, SharedFillState};
use super::setup_helpers::{fetch_initial_spots, fetch_validated_equity};

use crate::data::thetadata_live::{new_live_gex, start_live_gex, SharedLiveGex};

/// All shared state produced by [`LiveContext::setup`].
pub struct LiveContext {
    pub ibkr_client: Arc<ibapi::Client>,
    pub order_id_gen: Arc<OrderIdGen>,
    pub health_state: SharedHealthState,
    pub live_gex: SharedLiveGex,
    pub fill_state: SharedFillState,
    pub shutdown: Arc<std::sync::atomic::AtomicBool>,
    pub initial_equity: f64,
    pub spot_prices: HashMap<Ticker, f64>,
}

impl LiveContext {
    /// Connect broker, start health server, GEX stream, fill monitor, and fetch initial equity.
    pub async fn setup(
        tickers: &[Ticker],
        config: &StrategyConfig,
        mut broker: IbkrBroker,
        server_cfg: ServerConfig,
    ) -> Result<(Self, IbkrBroker)> {
        if tickers.is_empty() {
            anyhow::bail!("No tickers specified");
        }

        broker.connect().await?;
        println!("[live] IBKR broker connected");

        let ibkr_client = broker
            .client_arc()
            .expect("broker connected but no client");
        let order_id_gen = broker
            .order_id_gen()
            .expect("broker connected but no order ID generator");

        let health_state = new_health_state();
        {
            let mut s = super::lock_or_recover(&health_state);
            s.broker_connected = true;
        }
        prepopulate_tickers(&health_state, tickers);
        set_health_ibkr(&health_state, ibkr_client.clone());

        let hs = health_state.clone();
        tokio::spawn(async move {
            start_health_server(server_cfg, hs).await;
        });

        let interval = crate::config::BAR_INTERVAL_MINUTES;
        let max_pos = config.max_open_slots();
        println!(
            "[live] Bar interval: {}m | MaxOpen={} | {} tickers",
            interval, max_pos, tickers.len()
        );

        let spot_prices = fetch_initial_spots(tickers, &ibkr_client).await;

        let now_ms = crate::types::now_ms();
        for (&ticker, &spot) in &spot_prices {
            update_health(
                &health_state,
                ticker.as_str(),
                TickerHealth {
                    last_poll_ms: now_ms,
                    spot_price: spot,
                    warmup_status: Some("Queued for recovery".to_string()),
                    ..Default::default()
                },
            );
        }

        let live_gex = new_live_gex();
        set_health_gex(&health_state, live_gex.clone());

        let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
        start_live_gex(live_gex.clone(), tickers, shutdown.clone()).await?;

        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
        let initial_equity = fetch_validated_equity(&ibkr_client).await?;

        let fill_state = new_fill_state();
        {
            let fs = fill_state.clone();
            let client_for_monitor = ibkr_client.clone();
            tokio::spawn(async move {
                start_order_monitor(client_for_monitor, fs).await;
            });
        }

        Ok((
            Self { ibkr_client, order_id_gen, health_state, live_gex, fill_state, shutdown, initial_equity, spot_prices },
            broker,
        ))
    }
}
