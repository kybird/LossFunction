//! Money boundary conversion: exact Decimal KRW <-> INTEGER in 1e-4 KRW
//! units (KIS's own price precision). Round-half-even below 1e-4.

use rust_decimal::Decimal;
use rust_decimal::RoundingStrategy;

const MONEY_UNIT: Decimal = Decimal::from_parts(10_000, 0, 0, false, 0);

pub fn money_to_int(value: Decimal) -> i64 {
    let scaled =
        (value * MONEY_UNIT).round_dp_with_strategy(0, RoundingStrategy::MidpointNearestEven);
    i64::try_from(scaled.mantissa()).expect("money overflow beyond i64")
}

pub fn int_to_money(value: i64) -> Decimal {
    Decimal::from(value) / Decimal::from(10_000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn roundtrip_is_exact() {
        for value in ["80000", "80500.1234", "0.0001", "12345678.9999"] {
            let decimal = Decimal::from_str(value).unwrap();
            assert_eq!(int_to_money(money_to_int(decimal)), decimal);
        }
    }

    #[test]
    fn sub_unit_residues_round_half_even() {
        // 10000.5 -> 10000 (to even), 10001.5 -> 10002.
        assert_eq!(money_to_int(Decimal::from_str("1.00005").unwrap()), 10000);
        assert_eq!(money_to_int(Decimal::from_str("1.00015").unwrap()), 10002);
    }
}
