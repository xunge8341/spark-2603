use super::types::{RouteEntry, RouteKind};
use std::collections::HashMap;
use std::sync::RwLock;

pub type KindRoutes = HashMap<Box<str>, RouteEntry>;

/// Serializable/cloneable route set used in HostSpec.
pub type RouteSet = HashMap<RouteKind, KindRoutes>;

/// Dynamic route table (std-only).
#[derive(Debug, Default)]
pub struct RouteTable {
    routes: RwLock<RouteSet>,
}

impl RouteTable {
    pub fn new() -> Self {
        Self { routes: RwLock::new(HashMap::new()) }
    }

    pub fn replace_all(&self, routes: RouteSet) {
        let mut g = match self.routes.write() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        *g = routes;
    }

    pub fn insert(&self, kind: impl Into<RouteKind>, path: impl Into<Box<str>>, entry: RouteEntry) {
        let mut g = match self.routes.write() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        g.entry(kind.into()).or_default().insert(path.into(), entry);
    }

    pub fn lookup(&self, kind: &str, path: &str) -> Option<RouteEntry> {
        let g = match self.routes.read() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        g.get(kind).and_then(|m| m.get(path)).cloned()
    }
}
