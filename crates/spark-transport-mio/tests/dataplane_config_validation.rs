use spark_buffer::Bytes;
use spark_core::context::Context;
use spark_core::service::Service;
use spark_transport::policy::{FlushPolicy, WatermarkPolicy};
use spark_transport::{DataPlaneConfig, DataPlaneMetrics, KernelError};

use std::io::ErrorKind;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

struct Noop;

impl Service<Bytes> for Noop {
    type Response = Option<Bytes>;
    type Error = KernelError;

    async fn call(&self, _context: Context, _request: Bytes) -> Result<Self::Response, Self::Error> {
        Ok(None)
    }
}

#[test]
fn tcp_spawn_rejects_invalid_config_before_bind() {
    let cfg = DataPlaneConfig::default().with_watermark(WatermarkPolicy {
        min_frame: 1024,
        high_mul: 1,
        low_mul: 1,
    });

    let err = spark_transport_mio::try_spawn_tcp_dataplane(
        cfg,
        Arc::new(AtomicBool::new(false)),
        Arc::new(Noop),
        Arc::new(DataPlaneMetrics::default()),
    )
    .expect_err("invalid config must fail fast");

    assert_eq!(err.kind(), ErrorKind::InvalidInput);
}

#[test]
fn udp_spawn_rejects_invalid_config_before_socket_setup() {
    let cfg = DataPlaneConfig::default().with_flush_policy(FlushPolicy {
        min_frame: 64 * 1024,
        max_bytes_mul: 32,
        max_bytes_cap: 0,
        max_syscalls: 32,
        max_iov: 8,
    });

    let err = spark_transport_mio::try_spawn_udp_dataplane(
        cfg,
        "127.0.0.1:9".parse().expect("static addr"),
        Arc::new(AtomicBool::new(false)),
        Arc::new(Noop),
        Arc::new(DataPlaneMetrics::default()),
    )
    .expect_err("invalid config must fail fast");

    assert_eq!(err.kind(), ErrorKind::InvalidInput);
}
