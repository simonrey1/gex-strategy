use super::positions::Trade;
use ts_rs::TS;

#[derive(Debug, Clone, Default, serde::Serialize, TS)]
#[ts(export, export_to = "shared/generated/")]
#[serde(rename_all = "camelCase")]
pub struct TradeAnalysis {
    pub high_runup_losers: Vec<usize>,
    pub worst_losses: Vec<usize>,
}

impl TradeAnalysis {
    pub fn from_trades(trades: &[Trade]) -> Self {
        let mut high_runup: Vec<(usize, f64)> = trades.iter().enumerate()
            .filter(|(_, t)| t.net_pnl < 0.0 && t.max_runup_atr >= 2.0)
            .map(|(i, t)| (i, t.max_runup_atr))
            .collect();
        high_runup.sort_by(|a, b| crate::types::cmp_f64(b.1, a.1));

        let mut worst: Vec<(usize, f64)> = trades.iter().enumerate()
            .filter(|(_, t)| t.net_pnl < 0.0)
            .map(|(i, t)| (i, t.net_pnl))
            .collect();
        worst.sort_by(|a, b| crate::types::cmp_f64(a.1, b.1));

        Self {
            high_runup_losers: high_runup.into_iter().map(|(i, _)| i).collect(),
            worst_losses: worst.into_iter().map(|(i, _)| i).collect(),
        }
    }
}
