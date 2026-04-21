use anyhow::{Context, Result};
use chrono::Datelike;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};

use crate::config;
use crate::config::Ticker;
use crate::types::ToF64;
use crate::types::OhlcBar;

use super::types::{AccountSummary, BracketOrder, BracketOrderIds, Broker, MarketOrder};

/// Epoch-based order ID floor. Paper trading's nextValidId is unreliable
/// after gateway restarts (returns 1 but rejects it as duplicate).
fn epoch_order_id() -> i32 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Use last 9 digits of epoch to stay well within i32::MAX (2.1B)
    (secs % 1_000_000_000) as i32
}

/// Thread-safe order ID generator that bypasses ibapi's internal counter.
pub struct OrderIdGen(AtomicI32);

impl OrderIdGen {
    pub fn new(start: i32) -> Self {
        Self(AtomicI32::new(start))
    }
    pub fn next(&self) -> i32 {
        self.0.fetch_add(1, Ordering::SeqCst)
    }
}

pub struct IbkrBroker {
    client: Option<Arc<ibapi::Client>>,
    connected: bool,
    order_id_gen: Option<Arc<OrderIdGen>>,
}

impl Default for IbkrBroker {
    fn default() -> Self {
        Self { client: None, connected: false, order_id_gen: None }
    }
}

impl IbkrBroker {
    pub fn new() -> Self {
        Self::default()
    }

    fn client(&self) -> Result<&ibapi::Client> {
        self.client
            .as_ref()
            .map(|c| c.as_ref())
            .context("Not connected to IBKR")
    }

    pub fn client_arc(&self) -> Option<Arc<ibapi::Client>> {
        self.client.clone()
    }

    pub fn order_id_gen(&self) -> Option<Arc<OrderIdGen>> {
        self.order_id_gen.clone()
    }
}

fn time_to_chrono(t: time::OffsetDateTime) -> chrono::DateTime<chrono::Utc> {
    let ts = t.unix_timestamp();
    let ns = t.nanosecond();
    chrono::DateTime::from_timestamp(ts, ns).unwrap_or_else(chrono::Utc::now)
}

fn ibkr_bars_to_ohlc(bars: &[ibapi::market_data::historical::Bar]) -> Vec<OhlcBar> {
    let mut out: Vec<OhlcBar> = bars.iter().map(|b| OhlcBar {
        timestamp: time_to_chrono(b.date),
        open: b.open,
        high: b.high,
        low: b.low,
        close: b.close,
        volume: b.volume,
    }).collect();
    out.sort_by_key(|b| b.timestamp);
    out
}

const CONNECT_MAX_ATTEMPTS: u32 = 12;
const CONNECT_BASE_DELAY_MS: u64 = 2_000;
const CONNECT_MAX_DELAY_MS: u64 = 30_000;

impl Broker for IbkrBroker {
    fn connect(&mut self) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            let host = config::ibkr_host();
            let port = config::ibkr_port();
            let client_id = config::ibkr_client_id_live();
            let addr = format!("{}:{}", host, port);

            for attempt in 1..=CONNECT_MAX_ATTEMPTS {
                println!(
                    "[ibkr] Connecting to {} (client_id={}, attempt {}/{})",
                    addr, client_id, attempt, CONNECT_MAX_ATTEMPTS
                );

                match ibapi::Client::connect(&addr, client_id).await {
                    Ok(client) => {
                        let server_id = client.next_valid_order_id().await.unwrap_or(0);
                        let epoch_floor = epoch_order_id();
                        let start_id = server_id.max(epoch_floor);
                        println!(
                            "[ibkr] Connected (server_next_oid={}, epoch_floor={}, using={})",
                            server_id, epoch_floor, start_id
                        );
                        self.order_id_gen = Some(Arc::new(OrderIdGen::new(start_id)));
                        self.client = Some(Arc::new(client));
                        self.connected = true;
                        return Ok(());
                    }
                    Err(e) => {
                        if attempt == CONNECT_MAX_ATTEMPTS {
                            return Err(anyhow::anyhow!(
                                "Failed to connect to IBKR Gateway after {} attempts: {:?}",
                                CONNECT_MAX_ATTEMPTS,
                                e
                            ));
                        }
                        let delay = (CONNECT_BASE_DELAY_MS * 2u64.pow(attempt - 1))
                            .min(CONNECT_MAX_DELAY_MS);
                        eprintln!(
                            "[ibkr] Connection failed: {:?} — retrying in {:.0}s...",
                            e,
                            delay as f64 / 1000.0
                        );
                        tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                    }
                }
            }

            Err(anyhow::anyhow!("IBKR connect loop exited without result"))
        })
    }

    fn disconnect(&mut self) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            self.connected = false;
            self.client = None;
            println!("[ibkr] Disconnected");
            Ok(())
        })
    }

    fn is_connected(&self) -> bool {
        self.connected
    }

    fn get_account_summary(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<AccountSummary>> + Send + '_>> {
        Box::pin(async move {
            let client = self.client()?;

            use ibapi::accounts::types::AccountGroup;
            use ibapi::accounts::AccountSummaryResult as IbResult;

            let tags = &["NetLiquidation", "TotalCashValue"];
            let mut stream = client
                .account_summary(&AccountGroup("All".to_string()), tags)
                .await
                .context("account_summary request failed")?;

            let mut net_liq = 0.0_f64;
            let mut total_cash = 0.0_f64;

            let deadline = tokio::time::Instant::now()
                + tokio::time::Duration::from_secs(10);

            loop {
                match tokio::time::timeout_at(deadline, stream.next()).await {
                    Ok(Some(Ok(IbResult::Summary(summary)))) => {
                        let val: f64 = summary.value.parse().unwrap_or(0.0);
                        match summary.tag.as_str() {
                            "NetLiquidation" => net_liq = val,
                            "TotalCashValue" => total_cash = val,
                            _ => {}
                        }
                    }
                    Ok(Some(Ok(IbResult::End))) | Ok(None) => break,
                    Ok(Some(Err(e))) => {
                        tracing::warn!("[ibkr] account_summary error: {:?}", e);
                        break;
                    }
                    Err(_) => {
                        tracing::warn!("[ibkr] account_summary timed out after 10s");
                        break;
                    }
                }
            }

            Ok(AccountSummary {
                net_liquidation: net_liq,
                total_cash,
            })
        })
    }

    fn place_market_order(
        &self,
        order: MarketOrder,
    ) -> Pin<Box<dyn Future<Output = Result<i32>> + Send + '_>> {
        Box::pin(async move {
            let client = self.client()?;
            use ibapi::prelude::*;
            let contract = Contract::stock(order.ticker.as_str()).build();
            let qty = order.quantity.to_f64();

            let order_id = if order.action == "BUY" {
                client
                    .order(&contract)
                    .buy(qty)
                    .market()
                    .submit()
                    .await
                    .context("market buy failed")?
            } else {
                client
                    .order(&contract)
                    .sell(qty)
                    .market()
                    .submit()
                    .await
                    .context("market sell failed")?
            };

            let id: i32 = order_id.into();
            println!(
                "[ibkr] MARKET {} {} x{} -> orderId={}",
                order.action, order.ticker, order.quantity, id
            );
            Ok(id)
        })
    }

    fn place_bracket_order(
        &self,
        order: BracketOrder,
    ) -> Pin<Box<dyn Future<Output = Result<BracketOrderIds>> + Send + '_>> {
        Box::pin(async move {
            let client = self.client()?;
            use ibapi::prelude::*;
            let contract = Contract::stock(order.ticker.as_str()).build();
            let qty = order.quantity.to_f64();

            let ids = client
                .order(&contract)
                .buy(qty)
                .bracket()
                .entry_market()
                .take_profit(order.take_profit_price)
                .stop_loss(order.stop_loss_price)
                .submit_all()
                .await
                .context("bracket order failed")?;

            println!(
                "[ibkr] BRACKET {} {} x{} SL={:.2} TP={:.2} -> parent={}, tp={}, sl={}",
                order.action,
                order.ticker,
                order.quantity,
                order.stop_loss_price,
                order.take_profit_price,
                ids.parent,
                ids.take_profit,
                ids.stop_loss,
            );

            Ok(BracketOrderIds {
                parent_id: ids.parent.into(),
                stop_loss_id: ids.stop_loss.into(),
                take_profit_id: ids.take_profit.into(),
            })
        })
    }

    fn cancel_order(
        &self,
        order_id: i32,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            let client = self.client()?;
            let _ = client
                .cancel_order(order_id, "")
                .await
                .context("cancel_order failed")?;
            println!("[ibkr] CANCEL order {}", order_id);
            Ok(())
        })
    }
}

/// The `date` is a `chrono::NaiveDate`; we request bars ending at 20:00 UTC
/// (well past market close) with a 1-day duration.
const IBKR_REQUEST_TIMEOUT_SECS: u64 = 30;

/// Fetch **1-minute** bars for a specific historical date from IBKR Gateway (backtest cache).
pub async fn fetch_ibkr_bars_for_date(
    client: &ibapi::Client,
    ticker: Ticker,
    date: chrono::NaiveDate,
) -> Result<Vec<OhlcBar>> {
    use ibapi::market_data::historical::BarSize;
    use ibapi::prelude::*;

    let contract = Contract::stock(ticker.as_str()).build();

    let month = time::Month::try_from(date.month() as u8)
        .unwrap_or_else(|e| panic!("invalid month {} for {}: {}", date.month(), date, e));
    let end_dt = time::Date::from_calendar_date(date.year(), month, date.day() as u8)
        .unwrap_or_else(|e| panic!("invalid calendar date {}: {}", date, e))
        .with_hms(20, 0, 0)
        .expect("20:00:00 is always valid")
        .assume_utc();

    let bar_size = BarSize::Min;

    let data = tokio::time::timeout(
        std::time::Duration::from_secs(IBKR_REQUEST_TIMEOUT_SECS),
        client.historical_data(
            &contract,
            Some(end_dt),
            ibapi::market_data::historical::Duration::days(1),
            bar_size,
            Some(HistoricalWhatToShow::Trades),
            TradingHours::Regular,
        ),
    )
    .await
    .map_err(|_| anyhow::anyhow!("IBKR historical data timed out after {}s for {} {}", IBKR_REQUEST_TIMEOUT_SECS, ticker, date))?
    .context(format!("IBKR historical data for {} {}", ticker, date))?;

    Ok(ibkr_bars_to_ohlc(&data.bars))
}

/// Fetch today's bars at the strategy bar size (**15m**) from IBKR Gateway.
/// Returns an empty vec when IBKR has no data yet (e.g. right after market open).
pub async fn fetch_ibkr_intraday_bars(
    client: &ibapi::Client,
    ticker: Ticker,
) -> Result<Vec<OhlcBar>> {
    use ibapi::market_data::historical::BarSize;
    use ibapi::prelude::*;

    let contract = Contract::stock(ticker.as_str()).build();
    let bar_size = BarSize::Min15;

    let data = match tokio::time::timeout(
        std::time::Duration::from_secs(IBKR_REQUEST_TIMEOUT_SECS),
        client.historical_data(
            &contract,
            None,
            ibapi::market_data::historical::Duration::days(1),
            bar_size,
            Some(HistoricalWhatToShow::Trades),
            TradingHours::Regular,
        ),
    )
    .await
    {
        Ok(Ok(d)) => d,
        Ok(Err(e)) => {
            let msg = format!("{e:?}");
            if msg.contains("returned no data") || msg.contains("HMDS query returned no data") {
                return Ok(vec![]);
            }
            return Err(anyhow::anyhow!(e).context("IBKR historical data failed"));
        }
        Err(_) => {
            anyhow::bail!("IBKR intraday bars timed out after {}s for {}", IBKR_REQUEST_TIMEOUT_SECS, ticker);
        }
    };

    Ok(ibkr_bars_to_ohlc(&data.bars))
}

/// Fetch historical bars from IBKR for indicator warmup (multiple days) at **15m** bar size.
/// `end` = None → up to now. `end` = Some(date) → up to that date's close.
pub async fn fetch_ibkr_historical_bars(
    client: &ibapi::Client,
    ticker: Ticker,
    days: u32,
    end: Option<time::OffsetDateTime>,
) -> Result<Vec<OhlcBar>> {
    use ibapi::market_data::historical::BarSize;
    use ibapi::prelude::*;

    let contract = Contract::stock(ticker.as_str()).build();
    let bar_size = BarSize::Min15;
    let duration = ibapi::market_data::historical::Duration::days(
        i32::try_from(days).map_err(|_| {
            anyhow::anyhow!("[ibkr] historical days {} exceeds i32::MAX", days)
        })?,
    );

    let timeout_secs = if days > 30 { IBKR_REQUEST_TIMEOUT_SECS * 10 } else { IBKR_REQUEST_TIMEOUT_SECS };
    let hd = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        client.historical_data(
            &contract,
            end,
            duration,
            bar_size,
            Some(HistoricalWhatToShow::Trades),
            TradingHours::Regular,
        ),
    )
    .await
    .map_err(|_| anyhow::anyhow!("[ibkr] historical bars timed out after {}s for {} ({} days, 15m)", timeout_secs, ticker, days))?
    .with_context(|| format!("[ibkr] historical bars for {} ({} days, 15m)", ticker, days))?;

    Ok(ibkr_bars_to_ohlc(&hd.bars))
}
