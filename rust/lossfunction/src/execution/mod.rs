//! Execution layer — order lifecycle state and reconciliation.

pub mod state_machine;

pub use state_machine::{transitions, StateMachine, TransitionError, TransitionEvent};
