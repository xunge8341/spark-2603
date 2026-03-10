//! Mgmt routing (control-plane) utilities.
//!
//! Design notes (ASP.NET Core inspired):
//! - `MgmtApp` provides a minimal-API style surface: `map_get`, `map_post`, `map_group`.
//! - Routing tables are small / low-cardinality, so we optimize for clarity and DX.
//! - Lookup is `O(1)` by `(kind, path)` (no allocations) using `Box<str>` keys.
//!
//! **Note:** routing here is protocol-agnostic. `spark-ember` maps HTTP methods to route kinds.

mod app;
mod endpoint;
mod group;
mod request;
mod response;
mod table;
mod types;

pub use app::MgmtApp;
pub use endpoint::EndpointBuilder;
pub use group::MgmtGroup;
pub use request::{MgmtRequest, MgmtState};
pub use response::MgmtResponse;
pub use table::{RouteSet, RouteTable};
pub use types::{kinds, RouteEntry, RouteKind};
