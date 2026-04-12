pub mod auth;
pub mod dashboard_types;
pub mod equity;
pub mod dashboard;
pub mod init;
pub mod live_context;
pub mod live_entry_candidate;
pub mod live_poll_policy;
pub mod nyse_session;
pub mod orders;
pub mod reconcile;
pub mod recovery;
pub mod setup_helpers;
pub mod runner;
pub mod state;
pub mod ticker_state;
pub mod trade_log;

use std::sync::atomic::{AtomicBool, Ordering};

pub use crate::types::lock_or_recover;

static VERBOSE: AtomicBool = AtomicBool::new(false);

pub fn set_verbose(v: bool) {
    VERBOSE.store(v, Ordering::Relaxed);
}

pub fn is_verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}

macro_rules! log_debug {
    ($($arg:tt)*) => {
        if $crate::live::is_verbose() {
            println!($($arg)*);
        }
    };
}
pub(crate) use log_debug;
