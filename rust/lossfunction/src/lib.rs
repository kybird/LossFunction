//! LossFunction — open-source automated trading system for Korean equities.
//!
//! Rust port of the reference Python implementation; behavior is specified by
//! the project wiki (`doc/wiki/`): KIS endpoint contracts, order lifecycle
//! (UNKNOWN semantics), risk check ordering, and the SQLite conventions
//! (scaled-integer money, app-supplied UTC timestamps).

pub mod backtest;
pub mod broker;
pub mod config;
pub mod domain;
pub mod execution;
pub mod history;
pub mod risk;
pub mod runtime;
pub mod storage;
pub mod strategies;
pub mod strategy;
pub mod types;
