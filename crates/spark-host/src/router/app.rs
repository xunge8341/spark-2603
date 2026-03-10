use super::endpoint::EndpointBuilder;
use super::group::MgmtGroup;
use super::request::MgmtRequest;
use super::response::MgmtResponse;
use super::table::RouteSet;
use super::types::{kinds, MgmtHandlerFn, RouteEntry, RouteKind};

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// A minimal-API style app for mgmt endpoints (ASP.NET Core inspired).
#[derive(Debug, Default)]
pub struct MgmtApp {
    routes: RouteSet,
}

impl MgmtApp {
    pub fn new() -> Self {
        Self { routes: HashMap::new() }
    }

    pub fn routes(&self) -> &RouteSet {
        &self.routes
    }

    pub fn into_routes(self) -> RouteSet {
        self.routes
    }

    pub fn map_group<'a>(&'a mut self, prefix: impl Into<Box<str>>) -> MgmtGroup<'a> {
        MgmtGroup { app: self, prefix: prefix.into() }
    }

    pub fn map_get<'a, F, Fut>(&'a mut self, path: impl Into<Box<str>>, handler: F) -> EndpointBuilder<'a>
    where
        F: Fn(MgmtRequest) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = MgmtResponse> + Send + 'static,
    {
        self.map(kinds::GET, path, handler)
    }

    pub fn map_post<'a, F, Fut>(&'a mut self, path: impl Into<Box<str>>, handler: F) -> EndpointBuilder<'a>
    where
        F: Fn(MgmtRequest) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = MgmtResponse> + Send + 'static,
    {
        self.map(kinds::POST, path, handler)
    }

    pub fn map<'a, K, F, Fut>(&'a mut self, kind: K, path: impl Into<Box<str>>, handler: F) -> EndpointBuilder<'a>
    where
        K: Into<RouteKind>,
        F: Fn(MgmtRequest) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = MgmtResponse> + Send + 'static,
    {
        let kind: RouteKind = kind.into();
        let path: Box<str> = path.into();
        let default_id: Box<str> = path.clone();

        let h = Arc::new(move |req| -> Pin<Box<dyn Future<Output = MgmtResponse> + Send + 'static>> {
            Box::pin(handler(req))
        }) as Arc<MgmtHandlerFn>;

        let entry = RouteEntry { route_id: default_id, handler: h };

        let entry_mut = self
            .routes
            .entry(kind.clone())
            .or_default()
            .entry(path.clone())
            .or_insert(entry);

        EndpointBuilder { entry: entry_mut }
    }
}
