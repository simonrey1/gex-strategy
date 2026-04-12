use std::collections::HashMap;
use std::time::Duration;

use crate::config::Ticker;

const IBKR_SUBSCRIPTION_TIMEOUT: Duration = Duration::from_secs(10);

/// Snapshot of a single IBKR position for one symbol.
#[derive(Debug, Clone)]
pub struct IbkrPositionSnapshot {
    pub shares: f64,
    pub avg_cost: f64,
}

/// Snapshot of a single IBKR open order for one symbol.
#[derive(Debug, Clone)]
pub struct IbkrOrderSnapshot {
    pub order_id: i32,
    pub action: String,
    pub order_type: String,
    pub limit_price: Option<f64>,
    pub aux_price: Option<f64>,
    pub quantity: f64,
    pub status: String,
}

/// Fetch all IBKR positions, keyed by ticker symbol.
/// Returns `None` if the query failed or didn't complete (no `PositionEnd`).
pub async fn fetch_ibkr_positions(
    client: &ibapi::Client,
) -> Option<HashMap<String, IbkrPositionSnapshot>> {
    match tokio::time::timeout(IBKR_SUBSCRIPTION_TIMEOUT, fetch_ibkr_positions_inner(client)).await {
        Ok(result) => result,
        Err(_) => {
            eprintln!("[reconcile] positions() timed out after {:?}", IBKR_SUBSCRIPTION_TIMEOUT);
            None
        }
    }
}

async fn fetch_ibkr_positions_inner(
    client: &ibapi::Client,
) -> Option<HashMap<String, IbkrPositionSnapshot>> {
    let mut out: HashMap<String, IbkrPositionSnapshot> = HashMap::new();
    let mut completed = false;
    match client.positions().await {
        Ok(mut sub) => {
            while let Some(item) = sub.next().await {
                match item {
                    Ok(ibapi::accounts::PositionUpdate::Position(p)) => {
                        let sym = p.contract.symbol.to_string();
                        if p.position.abs() > 0.001 {
                            out.insert(sym, IbkrPositionSnapshot {
                                shares: p.position,
                                avg_cost: p.average_cost,
                            });
                        }
                    }
                    Ok(ibapi::accounts::PositionUpdate::PositionEnd) => {
                        completed = true;
                        break;
                    }
                    Err(e) => {
                        eprintln!("[reconcile] positions stream error: {:?}", e);
                        break;
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("[reconcile] positions() failed: {:?}", e);
        }
    }
    if completed { Some(out) } else { None }
}

/// Fetch all IBKR open orders, keyed by ticker symbol (may have multiple per symbol).
/// Returns `None` if the query failed or the stream errored before completing.
pub async fn fetch_ibkr_orders(
    client: &ibapi::Client,
) -> Option<HashMap<String, Vec<IbkrOrderSnapshot>>> {
    match tokio::time::timeout(IBKR_SUBSCRIPTION_TIMEOUT, fetch_ibkr_orders_inner(client)).await {
        Ok(result) => result,
        Err(_) => {
            eprintln!("[reconcile] all_open_orders() timed out after {:?}", IBKR_SUBSCRIPTION_TIMEOUT);
            None
        }
    }
}

async fn fetch_ibkr_orders_inner(
    client: &ibapi::Client,
) -> Option<HashMap<String, Vec<IbkrOrderSnapshot>>> {
    let mut out: HashMap<String, Vec<IbkrOrderSnapshot>> = HashMap::new();
    let mut completed = true;
    match client.all_open_orders().await {
        Ok(mut sub) => {
            while let Some(item) = sub.next().await {
                match item {
                    Ok(ibapi::orders::Orders::OrderData(od)) => {
                        let sym = od.contract.symbol.to_string();
                        out.entry(sym).or_default().push(IbkrOrderSnapshot {
                            order_id: od.order_id,
                            action: format!("{:?}", od.order.action),
                            order_type: od.order.order_type.clone(),
                            limit_price: od.order.limit_price,
                            aux_price: od.order.aux_price,
                            quantity: od.order.total_quantity,
                            status: od.order_state.status.clone(),
                        });
                    }
                    Ok(ibapi::orders::Orders::OrderStatus(_)) => {}
                    Ok(ibapi::orders::Orders::Notice(_)) => {}
                    Err(e) => {
                        eprintln!("[reconcile] open_orders stream error: {:?}", e);
                        completed = false;
                        break;
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("[reconcile] all_open_orders() failed: {:?}", e);
            completed = false;
        }
    }
    if completed { Some(out) } else { None }
}

/// From a list of IBKR orders for one symbol, find the SELL bracket pair (SL + TP).
/// When multiple STP orders exist (stale from previous ratchets), picks the highest
/// stop price (most recent ratchet) and returns the rest as extras to cancel.
pub fn find_bracket_orders(orders: &[IbkrOrderSnapshot]) -> (Option<&IbkrOrderSnapshot>, Option<&IbkrOrderSnapshot>, Vec<i32>) {
    let mut stops: Vec<&IbkrOrderSnapshot> = Vec::new();
    let mut tp: Option<&IbkrOrderSnapshot> = None;

    for o in orders {
        if o.action != "Sell" {
            continue;
        }
        let otype = o.order_type.to_uppercase();
        if otype.contains("STP") {
            stops.push(o);
        } else if otype.contains("LMT") && tp.is_none() {
            tp = Some(o);
        }
    }

    stops.sort_by(|a, b| {
        let pa = a.aux_price.unwrap_or(0.0);
        let pb = b.aux_price.unwrap_or(0.0);
        pb.partial_cmp(&pa).unwrap_or(std::cmp::Ordering::Equal)
    });

    let sl = stops.first().copied();
    let extra_stop_ids: Vec<i32> = stops.iter().skip(1).map(|o| o.order_id).collect();
    (sl, tp, extra_stop_ids)
}

/// Result of reconciling local state against IBKR for one ticker.
#[derive(Debug)]
pub enum ReconcileAction {
    /// Local and IBKR agree (both flat or both have matching position).
    Consistent,
    /// We think we're flat but IBKR has a position — adopt it.
    AdoptPosition {
        shares: f64,
        avg_cost: f64,
        sl_order_id: Option<i32>,
        sl_price: f64,
        tp_order_id: Option<i32>,
        tp_price: f64,
        extra_stop_ids: Vec<i32>,
    },
    /// We think we hold a position but IBKR says flat — position was closed externally.
    PositionGone,
    /// We hold a position and IBKR confirms it, but our bracket order IDs are stale/missing.
    /// `extra_stop_ids` are orphaned STPs from previous ratchets that should be cancelled.
    BracketStale {
        sl_order_id: Option<i32>,
        sl_price: f64,
        tp_order_id: Option<i32>,
        tp_price: f64,
        extra_stop_ids: Vec<i32>,
    },
    /// Both flat but IBKR has orphaned sell orders — need to cancel them.
    OrphanedOrders {
        order_ids: Vec<i32>,
    },
}

/// Local state snapshot for one ticker used during reconciliation.
pub struct LocalTickerView {
    pub ticker: Ticker,
    pub has_position: bool,
    pub bracket_sl: Option<i32>,
    pub bracket_tp: Option<i32>,
}

impl LocalTickerView {
    /// Compare local state for one ticker against IBKR snapshots and decide what to do.
    pub fn reconcile(
        &self,
        ibkr_positions: &HashMap<String, IbkrPositionSnapshot>,
        ibkr_orders: &HashMap<String, Vec<IbkrOrderSnapshot>>,
    ) -> ReconcileAction {
        let ticker = self.ticker;
        let sym = ticker.as_str();
        let ibkr_pos = ibkr_positions.get(sym);
        let ibkr_ords = ibkr_orders.get(sym);

    let ibkr_has_position = ibkr_pos.map(|p| p.shares >= 1.0).unwrap_or(false);

    match (self.has_position, ibkr_has_position) {
        (false, false) => {
            // Check for orphaned sell orders (e.g., TP/SL left over after position closed)
            if let Some(orders) = ibkr_ords {
                let orphaned: Vec<i32> = orders
                    .iter()
                    .filter(|o| o.action == "Sell")
                    .map(|o| o.order_id)
                    .collect();
                if !orphaned.is_empty() {
                    return ReconcileAction::OrphanedOrders { order_ids: orphaned };
                }
            }
            ReconcileAction::Consistent
        }

        (false, true) => {
            let p = ibkr_pos.expect("ibkr_has_position=true but ibkr_pos is None");
            let (sl, tp, extra) = ibkr_ords
                .map(|o| find_bracket_orders(o))
                .unwrap_or((None, None, vec![]));

            ReconcileAction::AdoptPosition {
                shares: p.shares,
                avg_cost: p.avg_cost,
                sl_order_id: sl.map(|o| o.order_id),
                sl_price: sl.and_then(|o| o.aux_price).unwrap_or(0.0),
                tp_order_id: tp.map(|o| o.order_id),
                tp_price: tp.and_then(|o| o.limit_price).unwrap_or(0.0),
                extra_stop_ids: extra,
            }
        }

        (true, false) => ReconcileAction::PositionGone,

        (true, true) => {
            let (sl, tp, extra) = ibkr_ords
                .map(|o| find_bracket_orders(o))
                .unwrap_or((None, None, vec![]));

            let ibkr_sl_id = sl.map(|o| o.order_id);
            let ibkr_tp_id = tp.map(|o| o.order_id);

            if ibkr_sl_id == self.bracket_sl && ibkr_tp_id == self.bracket_tp && extra.is_empty() {
                ReconcileAction::Consistent
            } else {
                ReconcileAction::BracketStale {
                    sl_order_id: ibkr_sl_id,
                    sl_price: sl.and_then(|o| o.aux_price).unwrap_or(0.0),
                    tp_order_id: ibkr_tp_id,
                    tp_price: tp.and_then(|o| o.limit_price).unwrap_or(0.0),
                    extra_stop_ids: extra,
                }
            }
        }
    }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(shares: f64, avg_cost: f64) -> IbkrPositionSnapshot {
        IbkrPositionSnapshot { shares, avg_cost }
    }

    fn order(id: i32, action: &str, otype: &str, limit: Option<f64>, aux: Option<f64>) -> IbkrOrderSnapshot {
        IbkrOrderSnapshot {
            order_id: id,
            action: action.to_string(),
            order_type: otype.to_string(),
            limit_price: limit,
            aux_price: aux,
            quantity: 10.0,
            status: "PreSubmitted".to_string(),
        }
    }

    fn local(has_pos: bool, sl: Option<i32>, tp: Option<i32>) -> LocalTickerView {
        LocalTickerView { ticker: Ticker::AAPL, has_position: has_pos, bracket_sl: sl, bracket_tp: tp }
    }

    #[test]
    fn both_flat_is_consistent() {
        let positions = HashMap::new();
        let orders = HashMap::new();
        let action = local(false, None, None).reconcile(&positions, &orders);
        assert!(matches!(action, ReconcileAction::Consistent));
    }

    #[test]
    fn ibkr_has_position_local_flat_adopts() {
        let mut positions = HashMap::new();
        positions.insert("AAPL".to_string(), pos(50.0, 150.0));

        let mut orders = HashMap::new();
        orders.insert("AAPL".to_string(), vec![
            order(101, "Sell", "STP", None, Some(140.0)),
            order(102, "Sell", "LMT", Some(170.0), None),
        ]);

        let action = local(false, None, None).reconcile(&positions, &orders);
        match action {
            ReconcileAction::AdoptPosition { shares, avg_cost, sl_order_id, tp_order_id, extra_stop_ids, .. } => {
                assert_eq!(shares, 50.0);
                assert_eq!(avg_cost, 150.0);
                assert_eq!(sl_order_id, Some(101));
                assert_eq!(tp_order_id, Some(102));
                assert!(extra_stop_ids.is_empty());
            }
            _ => panic!("expected AdoptPosition, got {:?}", action),
        }
    }

    #[test]
    fn local_has_position_ibkr_flat_gone() {
        let positions = HashMap::new();
        let orders = HashMap::new();
        let action = local(true, Some(10), Some(11)).reconcile(&positions, &orders);
        assert!(matches!(action, ReconcileAction::PositionGone));
    }

    #[test]
    fn both_hold_matching_brackets_consistent() {
        let mut positions = HashMap::new();
        positions.insert("AAPL".to_string(), pos(50.0, 150.0));

        let mut orders = HashMap::new();
        orders.insert("AAPL".to_string(), vec![
            order(101, "Sell", "STP", None, Some(140.0)),
            order(102, "Sell", "LMT", Some(170.0), None),
        ]);

        let action = local(true, Some(101), Some(102)).reconcile(&positions, &orders);
        assert!(matches!(action, ReconcileAction::Consistent));
    }

    #[test]
    fn both_hold_mismatched_brackets_stale() {
        let mut positions = HashMap::new();
        positions.insert("AAPL".to_string(), pos(50.0, 150.0));

        let mut orders = HashMap::new();
        orders.insert("AAPL".to_string(), vec![
            order(201, "Sell", "STP", None, Some(140.0)),
            order(202, "Sell", "LMT", Some(170.0), None),
        ]);

        let action = local(true, Some(101), Some(102)).reconcile(&positions, &orders);
        match action {
            ReconcileAction::BracketStale { sl_order_id, tp_order_id, .. } => {
                assert_eq!(sl_order_id, Some(201));
                assert_eq!(tp_order_id, Some(202));
            }
            _ => panic!("expected BracketStale, got {:?}", action),
        }
    }

    #[test]
    fn both_flat_with_orphaned_sell_orders() {
        let positions = HashMap::new();

        let mut orders = HashMap::new();
        orders.insert("AAPL".to_string(), vec![
            order(99, "Sell", "LMT", Some(170.0), None),
        ]);

        let action = local(false, None, None).reconcile(&positions, &orders);
        match action {
            ReconcileAction::OrphanedOrders { order_ids } => {
                assert_eq!(order_ids, vec![99]);
            }
            _ => panic!("expected OrphanedOrders, got {:?}", action),
        }
    }

    #[test]
    fn multiple_stops_picks_highest_and_returns_extras() {
        let mut positions = HashMap::new();
        positions.insert("AAPL".to_string(), pos(50.0, 150.0));

        let mut orders = HashMap::new();
        orders.insert("AAPL".to_string(), vec![
            order(3, "Sell", "STP", None, Some(140.0)),
            order(5, "Sell", "STP", None, Some(141.0)),
            order(10, "Sell", "STP", None, Some(143.0)),
            order(2, "Sell", "LMT", Some(170.0), None),
        ]);

        let action = local(true, Some(10), Some(2)).reconcile(&positions, &orders);
        match action {
            ReconcileAction::Consistent => {
                // SL=10 matches, TP=2 matches, but there are extras
                panic!("should not be Consistent with extra stops");
            }
            ReconcileAction::BracketStale { sl_order_id, extra_stop_ids, .. } => {
                assert_eq!(sl_order_id, Some(10));
                assert_eq!(extra_stop_ids, vec![5, 3]);
            }
            other => panic!("expected BracketStale, got {:?}", other),
        }
    }
}
