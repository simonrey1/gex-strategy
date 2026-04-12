use serde::{Deserialize, Serialize};
use std::fmt;
use ts_rs::TS;

macro_rules! define_tickers {
    ( $( $(#[$cat:meta])? $variant:ident => $str:literal ),+ $(,)? ) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
        #[ts(export, export_to = "shared/generated/")]
        pub enum Ticker { $( $variant ),+ }

        impl Ticker {
            pub const ALL: &[Ticker] = &[ $( Ticker::$variant ),+ ];

            pub fn as_str(self) -> &'static str {
                match self { $( Ticker::$variant => $str ),+ }
            }

            pub fn from_str_opt(s: &str) -> Option<Ticker> {
                match s { $( $str => Some(Ticker::$variant), )+ _ => None }
            }
        }
    };
}

define_tickers! {
    // Tech
    AAPL => "AAPL",
    GOOG => "GOOG",
    MSFT => "MSFT",
    // Banks / Financials
    JPM  => "JPM",
    BAC  => "BAC",
    GS   => "GS",
    WFC  => "WFC",
    MS   => "MS",
    AXP  => "AXP",
    C    => "C",
    // Consumer / Media
    DIS  => "DIS",
    HD   => "HD",
    WMT  => "WMT",
    LOW  => "LOW",
    MCD  => "MCD",
    // Consumer Staples
    PG   => "PG",
    KO   => "KO",
    PEP  => "PEP",
    CL   => "CL",
    COST => "COST",
    // Industrials
    CAT  => "CAT",
    HON  => "HON",
    DE   => "DE",
    UNP  => "UNP",
    // Healthcare
    JNJ  => "JNJ",
    UNH  => "UNH",
    MRK  => "MRK",
    PFE  => "PFE",
    LLY  => "LLY",
    ABBV => "ABBV",
    // Payments
    V    => "V",
    MA   => "MA",
    // Energy
    XOM  => "XOM",
    CVX  => "CVX",
    CCJ  => "CCJ",
    // Utilities
    NRG  => "NRG",
    SO   => "SO",
    NEE  => "NEE",
    DUK  => "DUK",
}

impl Ticker {
    /// Cache path for raw options-wide data (gzipped ThetaData snapshot).
    pub fn raw_wide_path(self, scope: &str, date: &str) -> std::path::PathBuf {
        let tag = format!("options_wide_v{}", crate::data::hist::VERSION_RAW);
        crate::data::cache::raw_path_for(scope, &tag, self.as_str(), date)
    }

    /// Whether WallBounce (calm-path) signal is enabled for this ticker.
    pub fn is_wb_enabled(self) -> bool {
        matches!(self, Ticker::JPM)
    }

    pub const STRATEGY: &[Ticker] = &[
        Ticker::AAPL, Ticker::GOOG, Ticker::MSFT, Ticker::JPM,
        Ticker::GS, Ticker::WMT, Ticker::HD, Ticker::DIS,
        Ticker::KO, Ticker::CAT, Ticker::MS, Ticker::NRG, Ticker::SO,
        Ticker::MCD, Ticker::COST
    ];
}

impl fmt::Display for Ticker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Ticker {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_str_opt(s).ok_or_else(|| anyhow::anyhow!("Unknown ticker: {}", s))
    }
}

pub const ALL_TICKERS_SYMBOL: &str = "ALL";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_str_roundtrip() {
        for &t in Ticker::ALL {
            let s = t.as_str();
            assert_eq!(Ticker::from_str_opt(s), Some(t));
        }
    }

    #[test]
    fn from_str_unknown() {
        assert_eq!(Ticker::from_str_opt("ZZZZ"), None);
    }

    #[test]
    fn display_matches_as_str() {
        assert_eq!(format!("{}", Ticker::AAPL), "AAPL");
        assert_eq!(format!("{}", Ticker::JPM), "JPM");
    }

    #[test]
    fn from_str_trait() {
        let t: Ticker = "GOOG".parse().unwrap();
        assert_eq!(t, Ticker::GOOG);
        assert!("INVALID".parse::<Ticker>().is_err());
    }

    #[test]
    fn strategy_subset_of_all() {
        for t in Ticker::STRATEGY {
            assert!(Ticker::ALL.contains(t), "{} in STRATEGY but not ALL", t);
        }
    }
}
