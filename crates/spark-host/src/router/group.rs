use super::endpoint::EndpointBuilder;
use super::request::MgmtRequest;
use super::response::MgmtResponse;
use super::types::{kinds, RouteKind};
use super::app::MgmtApp;

use std::future::Future;

/// A route group with a shared prefix.
pub struct MgmtGroup<'a> {
    pub(crate) app: &'a mut MgmtApp,
    pub(crate) prefix: Box<str>,
}

impl<'a> MgmtGroup<'a> {
    pub fn map_get<F, Fut>(&mut self, path: impl Into<Box<str>>, handler: F) -> EndpointBuilder<'_>
    where
        F: Fn(MgmtRequest) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = MgmtResponse> + Send + 'static,
    {
        let p = join_paths(&self.prefix, &path.into());
        self.app.map(kinds::GET, p, handler)
    }

    pub fn map_post<F, Fut>(&mut self, path: impl Into<Box<str>>, handler: F) -> EndpointBuilder<'_>
    where
        F: Fn(MgmtRequest) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = MgmtResponse> + Send + 'static,
    {
        let p = join_paths(&self.prefix, &path.into());
        self.app.map(kinds::POST, p, handler)
    }

    pub fn map_kind<K, F, Fut>(&mut self, kind: K, path: impl Into<Box<str>>, handler: F) -> EndpointBuilder<'_>
    where
        K: Into<RouteKind>,
        F: Fn(MgmtRequest) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = MgmtResponse> + Send + 'static,
    {
        let p = join_paths(&self.prefix, &path.into());
        self.app.map(kind, p, handler)
    }
}

fn join_paths(prefix: &str, path: &str) -> Box<str> {
    let prefix = prefix.trim_end_matches('/');
    let path = path.trim_start_matches('/');

    if prefix.is_empty() {
        return format!("/{path}").into_boxed_str();
    }
    if path.is_empty() {
        return format!("{prefix}/").into_boxed_str();
    }
    format!("{prefix}/{path}").into_boxed_str()
}
