use super::types::RouteEntry;

/// Fluent endpoint builder similar to ASP.NET Core's `RouteHandlerBuilder`.
pub struct EndpointBuilder<'a> {
    pub(crate) entry: &'a mut RouteEntry,
}

impl<'a> EndpointBuilder<'a> {
    pub fn named(self, id: impl Into<Box<str>>) -> Self {
        self.entry.route_id = id.into();
        self
    }
}
