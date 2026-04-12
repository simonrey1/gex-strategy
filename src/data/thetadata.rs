use anyhow::{Context, Result};
use chrono::NaiveDate;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::types::ToF64;

// ─── Response envelope types ────────────────────────────────────────────────
//
// ThetaData v3 wraps all JSON responses in {"response": [...]}.
// List endpoints return flat objects. Data endpoints return grouped:
//   {"response": [{"contract": {...}, "data": [...]}, ...]}

#[derive(Debug, Deserialize)]
struct FlatResponse<T> {
    response: Vec<T>,
}

#[derive(Debug, Deserialize)]
struct GroupedResponse<D> {
    response: Vec<ContractGroup<D>>,
}

#[derive(Debug, Deserialize)]
struct ContractGroup<D> {
    contract: ContractMeta,
    data: Vec<D>,
}

#[derive(Debug, Clone, Deserialize)]
struct ContractMeta {
    #[serde(default)]
    symbol: String,
    #[serde(default)]
    expiration: String,
    #[serde(default)]
    strike: f64,
    #[serde(default)]
    right: String,
}

// ─── Client ─────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct ThetaClient {
    http: Client,
    base_url: String,
}

const THETA_REQUEST_TIMEOUT_SECS: u64 = 300;

impl ThetaClient {
    pub fn new(host: &str, port: u16) -> Self {
        let http = Client::builder()
            .timeout(std::time::Duration::from_secs(THETA_REQUEST_TIMEOUT_SECS))
            .build()
            .unwrap_or_else(|_| Client::new());
        Self {
            http,
            base_url: format!("http://{}:{}/v3", host, port),
        }
    }

    pub fn from_env() -> Self {
        let host = crate::config::thetadata_host();
        let port = crate::config::thetadata_port();
        Self::new(&host, port)
    }

    /// Check for non-2xx status or plain-text error bodies from the terminal.
    async fn check_response(resp: reqwest::Response) -> Result<reqwest::Response> {
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("ThetaData HTTP {status}: {}", body.trim());
        }
        Ok(resp)
    }

    // ── Option listing ───────────────────────────────────────────────────

    pub async fn list_expirations(&self, symbol: &str) -> Result<Vec<NaiveDate>> {
        let url = format!("{}/option/list/expirations", self.base_url);
        let resp = self
            .http
            .get(&url)
            .query(&[("symbol", symbol), ("format", "json")])
            .send()
            .await
            .context("list_expirations request")?;
        let resp = Self::check_response(resp).await?;
        let envelope: FlatResponse<ExpirationRow> = resp
            .json()
            .await
            .context("list_expirations json")?;

        let dates: Vec<NaiveDate> = envelope
            .response
            .into_iter()
            .filter_map(|r| NaiveDate::parse_from_str(&r.expiration, crate::types::DATE_FMT).ok())
            .collect();
        Ok(dates)
    }

    // ── Option open interest (history, date range, all expirations) ─────

    pub async fn option_oi_history_range(
        &self,
        symbol: &str,
        start_date: NaiveDate,
        end_date: NaiveDate,
    ) -> Result<Vec<OiRow>> {
        let url = format!("{}/option/history/open_interest", self.base_url);
        let start_str = start_date.format("%Y%m%d").to_string();
        let end_str = end_date.format("%Y%m%d").to_string();
        let resp = self
            .http
            .get(&url)
            .query(&[
                ("symbol", symbol),
                ("expiration", "*"),
                ("start_date", &start_str),
                ("end_date", &end_str),
                ("format", "json"),
            ])
            .send()
            .await
            .context("option_oi_history_range request")?;
        let resp = Self::check_response(resp).await?;
        let envelope: GroupedResponse<OiDataRow> = resp
            .json()
            .await
            .context("option_oi_history_range json")?;
        Ok(flatten_oi(envelope))
    }

    // ── Option open interest (snapshot, all expirations) ─────────────────

    pub async fn option_oi_snapshot(
        &self,
        symbol: &str,
        max_dte: Option<u32>,
        strike_range: Option<u32>,
    ) -> Result<Vec<OiRow>> {
        let url = format!("{}/option/snapshot/open_interest", self.base_url);
        let mut params = vec![
            ("symbol", symbol.to_string()),
            ("expiration", "*".to_string()),
            ("format", "json".to_string()),
        ];
        if let Some(d) = max_dte {
            params.push(("max_dte", d.to_string()));
        }
        if let Some(sr) = strike_range {
            params.push(("strike_range", sr.to_string()));
        }

        let resp = self
            .http
            .get(&url)
            .query(&params)
            .send()
            .await
            .context("option_oi_snapshot request")?;
        let resp = Self::check_response(resp).await?;
        let envelope: GroupedResponse<OiDataRow> = resp
            .json()
            .await
            .context("option_oi_snapshot json")?;
        Ok(flatten_oi(envelope))
    }

    // ── Option Greeks history (date range, per-expiration) — Pro ────────

    async fn option_greeks_range_inner<D, R>(
        &self,
        url_suffix: &str,
        symbol: &str,
        expiration: NaiveDate,
        start_date: NaiveDate,
        end_date: NaiveDate,
        strike_range: Option<u32>,
        interval: &str,
        flatten: fn(GroupedResponse<D>) -> Vec<R>,
    ) -> Result<Vec<R>>
    where
        D: serde::de::DeserializeOwned,
    {
        let url = format!("{}/option/history/greeks/{}", self.base_url, url_suffix);
        let exp_str = expiration.format("%Y%m%d").to_string();
        let start_str = start_date.format("%Y%m%d").to_string();
        let end_str = end_date.format("%Y%m%d").to_string();

        let mut params = vec![
            ("symbol", symbol.to_string()),
            ("expiration", exp_str),
            ("start_date", start_str),
            ("end_date", end_str),
            ("interval", interval.to_string()),
            ("format", "json".to_string()),
        ];
        if let Some(sr) = strike_range {
            params.push(("strike_range", sr.to_string()));
        }

        let resp = self.http.get(&url).query(&params).send().await
            .context(format!("option_{url_suffix}_range request"))?;
        let resp = Self::check_response(resp).await?;
        let envelope: GroupedResponse<D> = resp.json().await
            .context(format!("option_{url_suffix}_range json"))?;
        Ok(flatten(envelope))
    }

    pub async fn option_second_order_range(
        &self,
        symbol: &str,
        expiration: NaiveDate,
        start_date: NaiveDate,
        end_date: NaiveDate,
        strike_range: Option<u32>,
        interval: &str,
    ) -> Result<Vec<SecondOrderGreeksRow>> {
        self.option_greeks_range_inner(
            "second_order", symbol, expiration, start_date, end_date,
            strike_range, interval, flatten_second_order,
        ).await
    }

    pub async fn option_all_greeks_range(
        &self,
        symbol: &str,
        expiration: NaiveDate,
        start_date: NaiveDate,
        end_date: NaiveDate,
        strike_range: Option<u32>,
        interval: &str,
    ) -> Result<Vec<AllGreeksRow>> {
        self.option_greeks_range_inner(
            "all", symbol, expiration, start_date, end_date,
            strike_range, interval, flatten_all_greeks,
        ).await
    }

    // ── Option all Greeks (snapshot, all expirations) — Pro ────────────

    pub async fn option_all_greeks_snapshot(
        &self,
        symbol: &str,
        max_dte: Option<u32>,
        strike_range: Option<u32>,
    ) -> Result<Vec<AllGreeksRow>> {
        let url = format!("{}/option/snapshot/greeks/all", self.base_url);
        let mut params = vec![
            ("symbol", symbol.to_string()),
            ("expiration", "*".to_string()),
            ("format", "json".to_string()),
            ("use_market_value", "true".to_string()),
        ];
        if let Some(d) = max_dte {
            params.push(("max_dte", d.to_string()));
        }
        if let Some(sr) = strike_range {
            params.push(("strike_range", sr.to_string()));
        }

        let resp = self
            .http
            .get(&url)
            .query(&params)
            .send()
            .await
            .context("option_all_greeks_snapshot request")?;
        let resp = Self::check_response(resp).await?;
        let envelope: GroupedResponse<AllGreeksDataRow> = resp
            .json()
            .await
            .context("option_all_greeks_snapshot json")?;
        Ok(flatten_all_greeks(envelope))
    }

}

// ─── Internal data-row types (inside "data" arrays) ─────────────────────────

#[derive(Debug, Deserialize)]
struct OiDataRow {
    #[serde(default)]
    timestamp: String,
    #[serde(default)]
    open_interest: i64,
}

#[derive(Debug, Deserialize)]
struct SecondOrderDataRow {
    #[serde(default)]
    timestamp: String,
    #[serde(default)]
    gamma: f64,
    #[serde(default)]
    vanna: f64,
    #[serde(default)]
    implied_vol: f64,
    #[serde(default)]
    underlying_price: f64,
}

#[derive(Debug, Deserialize)]
struct AllGreeksDataRow {
    #[serde(default)]
    timestamp: String,
    #[serde(default)]
    delta: f64,
    #[serde(default)]
    gamma: f64,
    #[serde(default)]
    theta: f64,
    #[serde(default)]
    vega: f64,
    #[serde(default)]
    rho: f64,
    #[serde(default)]
    vanna: f64,
    #[serde(default)]
    charm: f64,
    #[serde(default)]
    vomma: f64,
    #[serde(default)]
    implied_vol: f64,
    #[serde(default)]
    underlying_price: f64,
}

// ─── Flattening helpers (merge contract metadata into each data row) ────────

fn flatten_grouped<D, R>(
    envelope: GroupedResponse<D>,
    merge: fn(&ContractMeta, D) -> R,
) -> Vec<R> {
    let mut out = Vec::new();
    for group in envelope.response {
        let c = &group.contract;
        for d in group.data {
            out.push(merge(c, d));
        }
    }
    out
}

fn flatten_oi(envelope: GroupedResponse<OiDataRow>) -> Vec<OiRow> {
    flatten_grouped(envelope, |c, d| OiRow {
        symbol: c.symbol.clone(), expiration: c.expiration.clone(),
        strike: c.strike, right: c.right.clone(),
        timestamp: d.timestamp, open_interest: d.open_interest,
    })
}

fn flatten_second_order(envelope: GroupedResponse<SecondOrderDataRow>) -> Vec<SecondOrderGreeksRow> {
    flatten_grouped(envelope, |c, d| SecondOrderGreeksRow {
        symbol: c.symbol.clone(), expiration: c.expiration.clone(),
        strike: c.strike, right: c.right.clone(),
        timestamp: d.timestamp, gamma: d.gamma, vanna: d.vanna,
        implied_vol: d.implied_vol, underlying_price: d.underlying_price,
    })
}

fn flatten_all_greeks(envelope: GroupedResponse<AllGreeksDataRow>) -> Vec<AllGreeksRow> {
    flatten_grouped(envelope, |c, d| AllGreeksRow {
        symbol: c.symbol.clone(), expiration: c.expiration.clone(),
        strike: c.strike, right: c.right.clone(),
        timestamp: d.timestamp, delta: d.delta, gamma: d.gamma,
        theta: d.theta, vega: d.vega, rho: d.rho, vanna: d.vanna,
        charm: d.charm, vomma: d.vomma,
        implied_vol: d.implied_vol, underlying_price: d.underlying_price,
    })
}

// ─── Public response types (flattened, one row per contract per timestamp) ───

#[derive(Debug, Serialize, Deserialize)]
pub struct ExpirationRow {
    pub symbol: String,
    pub expiration: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OiRow {
    #[serde(default, skip_serializing)]
    pub symbol: String,
    pub expiration: String,
    pub strike: f64,
    pub right: String,
    #[serde(default, skip_serializing)]
    pub timestamp: String,
    pub open_interest: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecondOrderGreeksRow {
    #[serde(default, skip_serializing)]
    pub symbol: String,
    pub expiration: String,
    pub strike: f64,
    pub right: String,
    pub timestamp: String,
    #[serde(default)]
    pub gamma: f64,
    #[serde(default)]
    pub vanna: f64,
    #[serde(default)]
    pub implied_vol: f64,
    #[serde(default)]
    pub underlying_price: f64,
}

fn right_is_call(right: &str) -> bool {
    right.eq_ignore_ascii_case("call") || right == "C"
}

impl SecondOrderGreeksRow {
    pub fn is_call(&self) -> bool { right_is_call(&self.right) }
}

/// Flattened row from `greeks/all` endpoint.
/// Used for wide 15-min fetches to compute VEX, net dealer delta, and (future) charm/vanna exposure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllGreeksRow {
    #[serde(default, skip_serializing)]
    pub symbol: String,
    pub expiration: String,
    pub strike: f64,
    pub right: String,
    pub timestamp: String,
    #[serde(default)]
    pub delta: f64,
    #[serde(default)]
    pub gamma: f64,
    #[serde(default)]
    pub theta: f64,
    #[serde(default)]
    pub vega: f64,
    #[serde(default)]
    pub rho: f64,
    #[serde(default)]
    pub vanna: f64,
    #[serde(default)]
    pub charm: f64,
    #[serde(default)]
    pub vomma: f64,
    #[serde(default)]
    pub implied_vol: f64,
    #[serde(default)]
    pub underlying_price: f64,
}

impl AllGreeksRow {
    pub fn is_call(&self) -> bool { right_is_call(&self.right) }
}

impl OiRow {
    pub fn is_call(&self) -> bool { right_is_call(&self.right) }
}

// ─── Shared contract helpers ─────────────────────────────────────────────────

/// Canonical contract key: `"2025-02-14|235.00|P"`.
pub fn contract_key(expiration: &str, strike: f64, right: &str) -> String {
    let r = if right_is_call(right) { "C" } else { "P" };
    format!("{}|{:.2}|{}", expiration, strike, r)
}

/// Build an OI lookup from ThetaData OI rows, keeping the maximum value per
/// contract (ThetaData can return multiple snapshots per contract).
pub fn build_oi_map(oi_rows: &[OiRow]) -> std::collections::HashMap<String, f64> {
    let mut oi_map: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    for row in oi_rows {
        if row.open_interest > 0 {
            let key = contract_key(&row.expiration, row.strike, &row.right);
            let oi = row.open_interest.to_f64();
            oi_map.entry(key).and_modify(|v| *v = v.max(oi)).or_insert(oi);
        }
    }
    oi_map
}
