use super::request::MgmtRequest;
use super::response::MgmtResponse;
use std::borrow::Borrow;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

/// Route kind discriminator.
///
/// Spark's routing is **protocol-agnostic**: it routes control-plane requests by `(kind, path)`.
///
/// - For the default `spark-ember` HTTP/1.1 adapter, `kind` is the HTTP method ("GET", "POST", ...).
/// - For future adapters (serial/UDP/CLI), `kind` can be any stable discriminator.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct RouteKind(Box<str>);

impl RouteKind {
    #[inline]
    pub fn new(v: impl Into<Box<str>>) -> Self {
        Self(v.into())
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for RouteKind {
    #[inline]
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for RouteKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("RouteKind").field(&self.0).finish()
    }
}

impl From<&str> for RouteKind {
    #[inline]
    fn from(value: &str) -> Self {
        Self(value.to_string().into_boxed_str())
    }
}

impl From<String> for RouteKind {
    #[inline]
    fn from(value: String) -> Self {
        Self(value.into_boxed_str())
    }
}

impl From<Box<str>> for RouteKind {
    #[inline]
    fn from(value: Box<str>) -> Self {
        Self(value)
    }
}

/// Common route kinds used by the default HTTP control-plane adapter.
///
/// Note: Spark does **not** prescribe HTTP; this is just a convenient convention.
pub mod kinds {
    pub const GET: &str = "GET";
    pub const POST: &str = "POST";
    pub const PUT: &str = "PUT";
    pub const DELETE: &str = "DELETE";
}

pub type MgmtFuture = Pin<Box<dyn Future<Output = MgmtResponse> + Send + 'static>>;
pub type MgmtHandlerFn = dyn Fn(MgmtRequest) -> MgmtFuture + Send + Sync + 'static;

#[derive(Clone)]
pub struct RouteEntry {
    pub route_id: Box<str>,
    pub handler: Arc<MgmtHandlerFn>,
    pub request_timeout: Option<Duration>,
}

impl std::fmt::Debug for RouteEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RouteEntry")
            .field("route_id", &self.route_id)
            .field("handler", &"<handler>")
            .field("request_timeout", &self.request_timeout)
            .finish()
    }
}
