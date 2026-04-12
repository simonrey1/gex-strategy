use crate::types::Signal;

use super::signals::SignalState;

impl SignalState {
    /// Check exit conditions when holding a position.
    /// VannaFlip exits purely via SL/TP bracket + wall-trailing SL.
    /// Returns `None` always — signal-level exits have been removed for simplicity.
    ///
    /// exits cancelled the IBKR TP bracket prematurely and produced worse returns
    /// than letting the bracket ride. Wall-trailing SL handles downside; TP handles upside.
    pub fn check_exit(&self) -> Option<crate::types::TradeSignal> {
        match self.holding {
            Signal::Flat => None,
            Signal::LongVannaFlip | Signal::LongWallBounce => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_returns_none() {
        let s = SignalState::default();
        assert!(s.check_exit().is_none());
    }

    #[test]
    fn holding_returns_none() {
        let mut s = SignalState::default();
        s.holding = Signal::LongVannaFlip;
        assert!(s.check_exit().is_none());
    }
}
