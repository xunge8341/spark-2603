use super::types::RouteEntry;

use std::time::Duration;

/// Fluent endpoint builder similar to ASP.NET Core's `RouteHandlerBuilder`.
pub struct EndpointBuilder<'a> {
    pub(crate) entry: &'a mut RouteEntry,
}

impl<'a> EndpointBuilder<'a> {
    pub fn named(self, id: impl Into<Box<str>>) -> Self {
        self.entry.route_id = id.into();
        self
    }

    pub fn with_request_timeout(self, timeout: Duration) -> Self {
        self.entry.request_timeout = Some(timeout);
        self
    }
}
