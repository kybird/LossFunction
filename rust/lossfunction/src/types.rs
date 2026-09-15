//! Foundational shared-kernel types.
//!
//! Prices are exact `Decimal` (KRW); quantities are whole shares (Korean
//! equities do not trade fractionally); symbols are 6-digit domestic tickers.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Exact KRW price. KIS reports prices with up to 4 decimals.
pub type Price = Decimal;

/// Share count.
pub type Quantity = i64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrderSide {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrderType {
    Market,
    Limit,
}

/// A validated 6-digit domestic ticker (e.g. `"005930"`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Symbol(String);

#[derive(Debug, thiserror::Error)]
#[error("symbol must be exactly 6 digits, got {0:?}")]
pub struct InvalidSymbol(String);

impl Symbol {
    pub fn parse(value: impl Into<String>) -> Result<Self, InvalidSymbol> {
        let value = value.into();
        if value.len() == 6 && value.bytes().all(|b| b.is_ascii_digit()) {
            Ok(Self(value))
        } else {
            Err(InvalidSymbol(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_accepts_six_digits() {
        assert_eq!(Symbol::parse("005930").unwrap().as_str(), "005930");
    }

    #[test]
    fn symbol_rejects_other_shapes() {
        for bad in ["AAPL", "0059", "0059300", "0593Oa"] {
            assert!(Symbol::parse(bad).is_err(), "{bad} should be rejected");
        }
    }

    #[test]
    fn sides_and_types_serialize_lowercase() {
        assert_eq!(serde_json::to_string(&OrderSide::Buy).unwrap(), "\"buy\"");
        assert_eq!(
            serde_json::to_string(&OrderType::Limit).unwrap(),
            "\"limit\""
        );
    }
}
