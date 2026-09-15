//! LossFunction — open-source automated trading system for Korean equities.
//!
//! Rust port of the reference Python implementation; behavior is specified by
//! the project wiki (`doc/wiki/`): KIS endpoint contracts, order lifecycle
//! (UNKNOWN semantics), risk check ordering, and the SQLite conventions
//! (scaled-integer money, app-supplied UTC timestamps).

pub mod config;
pub mod types;
