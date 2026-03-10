/// Marker trait for assembled applications/pipelines.
///
/// Concrete APIs (IO, routing, etc.) will be added incrementally.
pub trait App: Send + Sync + 'static {}
impl<T> App for T where T: Send + Sync + 'static {}
