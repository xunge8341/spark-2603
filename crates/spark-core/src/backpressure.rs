/// Backpressure state machine contract placeholder.
///
/// Implementation lives in adapter/server; core only defines semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackpressureState {
    Inactive,
    Active,
}
