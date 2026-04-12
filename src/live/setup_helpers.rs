use std::collections::HashMap;

use anyhow::Result;

use crate::broker::ibkr::fetch_ibkr_intraday_bars;
use crate::config::Ticker;

use super::equity::AccountEquity;
use super::log_debug;
use super::live_poll_policy::LivePollPolicy;

/// Fetch spot prices for all tickers from IBKR.
pub(crate) async fn fetch_initial_spots(
    tickers: &[Ticker],
    client: &ibapi::Client,
) -> HashMap<Ticker, f64> {
    let mut spot_prices = HashMap::new();
    for &ticker in tickers {
        let result = tokio::time::timeout(
            tokio::time::Duration::from_millis(LivePollPolicy::FETCH_TIMEOUT_MS),
            fetch_ibkr_intraday_bars(client, ticker),
        ).await;
        match result {
            Ok(Ok(bars)) => {
                if let Some(last) = bars.last() {
                    spot_prices.insert(ticker, last.close);
                    log_debug!(
                        "[live] {} spot: ${:.2} ({} {}m bars)",
                        ticker, last.close, bars.len(), crate::config::BAR_INTERVAL_MINUTES,
                    );
                } else {
                    log_debug!("[live] {} no bars yet (market may have just opened)", ticker);
                }
            }
            Ok(Err(e)) => {
                log_debug!(
                    "[live] Could not fetch spot for {}: {:?} — GEX filter will use full strike range",
                    ticker, e
                );
            }
            Err(_) => {
                eprintln!(
                    "[live] {} spot fetch timed out ({}s) — will retry on first bar poll",
                    ticker, LivePollPolicy::FETCH_TIMEOUT_MS / 1000
                );
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(350)).await;
    }
    spot_prices
}

/// Fetch and validate USD account equity from IBKR.
pub(crate) async fn fetch_validated_equity(client: &ibapi::Client) -> Result<f64> {
    match AccountEquity::fetch(client).await {
        Some(eq) => {
            println!(
                "[live] Account NetLiq: {:.0} {} | USD available: ${:.0}",
                eq.net_liq, eq.net_liq_currency,
                eq.usd_available.unwrap_or(0.0)
            );
            match eq.usd_available {
                Some(usd) if usd > 0.0 => Ok(usd),
                Some(_) => anyhow::bail!(
                    "[live] USD available is zero — cannot size positions. \
                     NetLiq is {:.0} {}. Convert EUR→USD in IBKR before trading.",
                    eq.net_liq, eq.net_liq_currency
                ),
                None => anyhow::bail!(
                    "[live] No USD funds found in account — cannot size positions. \
                     NetLiq is {:.0} {}. Convert EUR→USD in IBKR before trading.",
                    eq.net_liq, eq.net_liq_currency
                ),
            }
        }
        None => anyhow::bail!(
            "[live] Could not fetch account equity from IBKR — aborting. \
             Check that the IBKR gateway is running and account data is available."
        ),
    }
}
