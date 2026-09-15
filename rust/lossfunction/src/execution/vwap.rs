//! VWAP execution algorithm — splits a large parent order into child slices
//! that track the volume-weighted average price (wiki: quant-strategies).
//!
//! The scheduler is deterministic and pure: given the elapsed schedule
//! progress and realized fills, it returns the next child slice. Risk
//! enforcement stays upstream (the gateway path is unchanged).

use rust_decimal::Decimal;

/// Even-volume VWAP slicing over `slices` equal buckets.
pub struct VwapSchedule {
    total_quantity: i64,
    slices: i64,
    filled: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum VwapError {
    #[error("total quantity {quantity} is not divisible by {slices} slices")]
    Indivisible { quantity: i64, slices: i64 },
    #[error("slices must be positive, got {0}")]
    NonPositiveSlices(i64),
}

impl VwapSchedule {
    pub fn new(total_quantity: i64, slices: i64) -> Result<Self, VwapError> {
        if slices <= 0 {
            return Err(VwapError::NonPositiveSlices(slices));
        }
        if total_quantity % slices != 0 {
            return Err(VwapError::Indivisible {
                quantity: total_quantity,
                slices,
            });
        }
        Ok(Self {
            total_quantity,
            slices,
            filled: 0,
        })
    }

    /// Quantity for the next child slice (zero when complete).
    pub fn next_slice(&self) -> i64 {
        if self.filled >= self.total_quantity {
            0
        } else {
            self.total_quantity / self.slices
        }
    }

    /// Record a child fill; saturates at the parent total.
    pub fn record_fill(&mut self, quantity: i64) {
        self.filled = (self.filled + quantity).min(self.total_quantity);
    }

    /// Realized fill / total, exact Decimal.
    pub fn progress(&self) -> Decimal {
        Decimal::from(self.filled) / Decimal::from(self.total_quantity)
    }

    pub fn complete(&self) -> bool {
        self.filled >= self.total_quantity
    }
}

/// VWAP price over (price, volume) bars — exact Decimal.
pub fn vwap(bars: &[(Decimal, i64)]) -> Option<Decimal> {
    let total_volume: i64 = bars.iter().map(|(_, volume)| *volume).sum();
    if total_volume <= 0 {
        return None;
    }
    let notional: Decimal = bars
        .iter()
        .map(|(price, volume)| *price * Decimal::from(*volume))
        .sum();
    Some(notional / Decimal::from(total_volume))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn schedule_splits_evenly_and_tracks_progress() {
        let mut schedule = VwapSchedule::new(1000, 10).unwrap();
        assert_eq!(schedule.next_slice(), 100);
        assert_eq!(schedule.progress(), Decimal::ZERO);

        for _ in 0..10 {
            let slice = schedule.next_slice();
            assert_eq!(slice, 100);
            schedule.record_fill(slice);
        }
        assert!(schedule.complete());
        assert_eq!(schedule.progress(), Decimal::ONE);
        assert_eq!(schedule.next_slice(), 0); // no overshoot
    }

    #[test]
    fn indivisible_or_invalid_rejected() {
        assert!(matches!(
            VwapSchedule::new(1001, 10),
            Err(VwapError::Indivisible { .. })
        ));
        assert!(matches!(
            VwapSchedule::new(100, 0),
            Err(VwapError::NonPositiveSlices(0))
        ));
    }

    #[test]
    fn saturation_prevents_overshoot() {
        let mut schedule = VwapSchedule::new(100, 4).unwrap();
        schedule.record_fill(90);
        schedule.record_fill(50); // would overshoot
        assert_eq!(schedule.filled, 100);
        assert!(schedule.complete());
    }

    #[test]
    fn vwap_matches_hand_computation() {
        let bars = [(Decimal::from(10_000), 3), (Decimal::from(20_000), 1)];
        // (30000 + 20000) / 4 = 12500
        assert_eq!(vwap(&bars), Some(Decimal::from(12_500)));
        assert_eq!(vwap(&[(Decimal::ONE, 0)]), None);
    }

    #[test]
    fn exact_fractional_vwap() {
        let bars = [
            (Decimal::from_str("100.5").unwrap(), 7),
            (Decimal::from_str("200.25").unwrap(), 3),
        ];
        // (703.5 + 600.75) / 10 = 130.425
        assert_eq!(vwap(&bars), Some(Decimal::from_str("130.425").unwrap()));
    }
}
