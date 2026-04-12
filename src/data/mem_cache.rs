use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use crate::config::Ticker;
use crate::types::{GexProfile, OhlcBar};

type GexMap = HashMap<i64, GexProfile>;

type GexStore = Mutex<HashMap<(Ticker, String), Arc<GexMap>>>;
type BarsStore = Mutex<HashMap<(Ticker, String), Arc<Vec<OhlcBar>>>>;

static GEX: OnceLock<GexStore> = OnceLock::new();
static BARS: OnceLock<BarsStore> = OnceLock::new();
static BARS_15M: OnceLock<BarsStore> = OnceLock::new();

fn bars_15m_store() -> &'static BarsStore {
    BARS_15M.get_or_init(|| Mutex::new(HashMap::new()))
}

fn gex_store() -> &'static GexStore {
    GEX.get_or_init(|| Mutex::new(HashMap::new()))
}

fn bars_store() -> &'static BarsStore {
    BARS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn get_gex(ticker: Ticker, month: &str) -> Option<Arc<GexMap>> {
    let cache = gex_store().lock().unwrap();
    cache.get(&(ticker, month.to_string())).cloned()
}

pub fn put_gex(ticker: Ticker, month: &str, data: GexMap) {
    gex_store().lock().unwrap().insert((ticker, month.to_string()), Arc::new(data));
}

pub fn get_bars(ticker: Ticker, date: &str) -> Option<Arc<Vec<OhlcBar>>> {
    let cache = bars_store().lock().unwrap();
    cache.get(&(ticker, date.to_string())).cloned()
}

pub fn get_bars_15m(ticker: Ticker, date: &str) -> Option<Arc<Vec<OhlcBar>>> {
    let cache = bars_15m_store().lock().unwrap();
    cache.get(&(ticker, date.to_string())).cloned()
}

pub fn put_bars_15m(ticker: Ticker, date: &str, data: Vec<OhlcBar>) {
    bars_15m_store().lock().unwrap().insert((ticker, date.to_string()), Arc::new(data));
}

pub fn put_bars(ticker: Ticker, date: &str, data: Vec<OhlcBar>) {
    bars_store().lock().unwrap().insert((ticker, date.to_string()), Arc::new(data));
}

pub fn put_bars_bulk(entries: Vec<(Ticker, String, Vec<OhlcBar>)>) {
    let mut cache = bars_store().lock().unwrap();
    for (ticker, date, data) in entries {
        cache.insert((ticker, date), Arc::new(data));
    }
}

pub fn put_gex_bulk(entries: Vec<(Ticker, String, GexMap)>) {
    let mut cache = gex_store().lock().unwrap();
    for (ticker, month, data) in entries {
        cache.insert((ticker, month), Arc::new(data));
    }
}

pub fn evict_gex(ticker: Ticker, month: &str) {
    gex_store().lock().unwrap().remove(&(ticker, month.to_string()));
}
