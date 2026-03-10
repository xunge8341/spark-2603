use crate::context::Context;

/// Data-plane service contract.
///
/// - Native async in trait (Rust 1.75+).
/// - Intended for static dispatch via generics.
#[allow(async_fn_in_trait)]
pub trait Service<Request>: Send + Sync + 'static {
    type Response: Send + 'static;
    type Error: Send + 'static;

    async fn call(&self, context: Context, request: Request)
        -> Result<Self::Response, Self::Error>;
}
