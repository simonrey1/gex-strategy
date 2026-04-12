use std::collections::HashMap;

use crate::config::Ticker;
use crate::types::OhlcBar;

pub type DayData = (
    HashMap<Ticker, Vec<OhlcBar>>,
    HashMap<Ticker, Vec<OhlcBar>>,
);
