//! Data-plane routing utilities.
//!
//! `no_std` and allocation-free (static dispatch).

use crate::{Context, Service};

/// Selector for 2-way routing.
///
/// Return `true` to choose `a`, `false` to choose `b`.
pub trait Select2<Req>: Send + Sync + 'static {
    fn select(&self, ctx: &Context, req: &Req) -> bool;
}

/// A 2-way router service.
///
/// This keeps everything statically typed (no dyn, no maps) and therefore
/// works well with native async traits.
pub struct Router2<S, A, B> {
    pub selector: S,
    pub a: A,
    pub b: B,
    pub route_a: &'static str,
    pub route_b: &'static str,
}

impl<S, A, B> Router2<S, A, B> {
    pub fn new(selector: S, a: A, b: B, route_a: &'static str, route_b: &'static str) -> Self {
        Self { selector, a, b, route_a, route_b }
    }
}

impl<Req, S, A, B> Service<Req> for Router2<S, A, B>
where
    Req: Send + 'static,
    S: Select2<Req>,
    A: Service<Req, Response = B::Response, Error = B::Error> + Send + Sync + 'static,
    B: Service<Req> + Send + Sync + 'static,
    B::Response: Send + 'static,
    B::Error: Send + 'static,
{
    type Response = B::Response;
    type Error = B::Error;

    async fn call(&self, mut context: Context, request: Req) -> Result<Self::Response, Self::Error> {
        let choose_a = self.selector.select(&context, &request);
        if choose_a {
            context.route_id = Some(self.route_a);
            self.a.call(context, request).await
        } else {
            context.route_id = Some(self.route_b);
            self.b.call(context, request).await
        }
    }
}
