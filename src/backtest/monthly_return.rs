use ts_rs::TS;

#[derive(Debug, Clone, serde::Serialize, TS)]
#[ts(export, export_to = "shared/generated/")]
pub struct MonthlyReturn {
    pub month: String,
    pub pnl: f64,
    #[serde(rename = "returnPct")]
    pub return_pct: f64,
    pub trades: u32,
}
