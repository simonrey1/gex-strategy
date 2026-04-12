use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::live::log_debug;
use crate::types::lock_or_recover;

#[derive(Debug, Clone)]
pub struct OrderFill {
    pub order_id: i32,
    pub filled: f64,
    pub avg_price: f64,
    pub status: String,
}

pub type SharedFillState = Arc<Mutex<HashMap<i32, OrderFill>>>;

pub fn new_fill_state() -> SharedFillState {
    Arc::new(Mutex::new(HashMap::new()))
}

pub fn take_fill(state: &SharedFillState, order_id: i32) -> Option<OrderFill> {
    lock_or_recover(state).remove(&order_id)
}

pub async fn start_order_monitor(client: Arc<ibapi::Client>, fill_state: SharedFillState) {
    use ibapi::orders::OrderUpdate;

    let mut consecutive_failures: u32 = 0;

    loop {
        if !client.is_connected() {
            eprintln!("[ibkr] Order monitor: client disconnected — backing off");
            tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
            continue;
        }

        let stream = match client.order_update_stream().await {
            Ok(s) => {
                consecutive_failures = 0;
                s
            }
            Err(e) => {
                consecutive_failures += 1;
                let delay = 10u64.min(5 * u64::from(consecutive_failures));
                if consecutive_failures <= 3 {
                    eprintln!(
                        "[ibkr] Failed to start order update stream: {:?} — retrying in {}s",
                        e, delay
                    );
                }
                tokio::time::sleep(tokio::time::Duration::from_secs(delay)).await;
                continue;
            }
        };

        log_debug!("[ibkr] Order update stream started");
        let mut stream = stream;
        while let Some(update) = stream.next().await {
            match update {
                Ok(OrderUpdate::OrderStatus(status)) => {
                    let is_terminal =
                        status.status == "Filled" || status.status == "Cancelled";
                    if is_terminal {
                        let mut s = lock_or_recover(&fill_state);
                        s.insert(
                            status.order_id,
                            OrderFill {
                                order_id: status.order_id,
                                filled: status.filled,
                                avg_price: status.average_fill_price,
                                status: status.status.clone(),
                            },
                        );
                        log_debug!(
                            "[ibkr] Order {} → {} ({} filled @ {:.2})",
                            status.order_id, status.status, status.filled, status.average_fill_price
                        );
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    eprintln!("[ibkr] Order update stream error: {:?}", e);
                    break;
                }
            }
        }
        consecutive_failures += 1;
        let delay = 5u64.min(5 * u64::from(consecutive_failures));
        if consecutive_failures <= 3 {
            eprintln!("[ibkr] Order update stream ended — reconnecting in {}s", delay);
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(delay)).await;
    }
}
