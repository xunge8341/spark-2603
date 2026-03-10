//! Ember 的运行入口（运行时中立）。
//!
//! 说明：
//! - `spark-host` 产出的 `HostSpec` 仅描述装配结果；
//! - 控制面 server 可以是 std-only（默认），也可以是 transport-backed（dogfooding profile）；
//! - 数据面由调用方注入（闭包/函数）：这样 ember 不会绑定到 mio/tokio。

use crate::server::EmberState;
use spark_host::builder::HostSpec;
use spark_host::router::RouteTable;
#[cfg(feature = "transport-mgmt")]
use spark_transport::DataPlaneMetrics;

#[cfg(feature = "transport-mgmt")]
use crate::http1::{HttpMgmtService, SpawnedTcpDataplane};

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread::JoinHandle;

/// 以“控制面阻塞 + 数据面线程”的方式运行一个 HostSpec。
///
/// `spawn_dataplane` 由调用方提供：
/// - 典型实现是调用 `spark-transport-mio::spawn_tcp_dataplane(...)`；
/// - 或者未来的 io_uring / tokio 等后端。
///
/// 当启用 `transport-mgmt` feature 时，控制面默认使用 Spark transport 实现（dogfooding）。
#[cfg(feature = "transport-mgmt")]
pub fn run_blocking<S, SpawnDp, SpawnMgmt>(
    spec: HostSpec<S>,
    spawn_dataplane: SpawnDp,
    spawn_mgmt: SpawnMgmt,
) -> std::io::Result<()>
where
    SpawnDp: FnOnce(&HostSpec<S>, Arc<AtomicBool>) -> std::io::Result<JoinHandle<()>>,
    SpawnMgmt: FnOnce(
        spark_transport::DataPlaneConfig,
        Arc<AtomicBool>,
        Arc<HttpMgmtService>,
        Arc<DataPlaneMetrics>,
    ) -> std::io::Result<SpawnedTcpDataplane>,
{
    // 控制面路由表。
    let routes = Arc::new(RouteTable::new());
    routes.replace_all(spec.mgmt.clone());

    let state = Arc::new(EmberState::new());
    let draining = state.draining_handle();

    // 数据面启动（由调用方决定后端）。
    let dp_handle = spawn_dataplane(&spec, Arc::clone(&draining))?;


    let res: std::io::Result<()> = {
        {
            // 控制面：transport-backed server（dogfooding）。
            let mgmt_profile = spec.config.mgmt_profile_v1();
            let mgmt_metrics = Arc::new(DataPlaneMetrics::default());
            let server = crate::http1::TransportServer::new(mgmt_profile, routes, state, mgmt_metrics);
            let mgmt = server.try_spawn_with(spawn_mgmt)?;

            let res = mgmt
                .join
                .join()
                .map_err(|_e| std::io::Error::other("mgmt dataplane thread panicked"));

            draining.store(true, std::sync::atomic::Ordering::Release);
            let _ = dp_handle.join();
            res
        }

    };

    res
}

/// Non-dogfooding entry: std-only mgmt plane.
#[cfg(not(feature = "transport-mgmt"))]
pub fn run_blocking<S, SpawnDp>(spec: HostSpec<S>, spawn_dataplane: SpawnDp) -> std::io::Result<()>
where
    SpawnDp: FnOnce(&HostSpec<S>, Arc<AtomicBool>) -> std::io::Result<JoinHandle<()>>,
{
    // 控制面路由表。
    let routes = Arc::new(RouteTable::new());
    routes.replace_all(spec.mgmt.clone());

    let state = Arc::new(EmberState::new());
    let draining = state.draining_handle();

    // 数据面启动（由调用方决定后端）。
    let dp_handle = spawn_dataplane(&spec, Arc::clone(&draining))?;

    // 控制面：std-only server。
    let server = crate::server::Server::new(spec.config.clone(), routes, state);
    let res = server.serve();

    draining.store(true, std::sync::atomic::Ordering::Release);
    let _ = dp_handle.join();
    res
}
