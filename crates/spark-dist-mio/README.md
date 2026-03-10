# spark-dist-mio

默认发行物：**mio 数据面 + ember 控制面**。

目标：新同学“一页 README 就能跑”。同时保持主干（`spark-transport`）零污染。

## 最短可运行示例（TCP）

```rust
use spark_buffer::Bytes;
use spark_core::context::Context;
use spark_core::service::Service;
use spark_host::builder::HostBuilder;
use spark_transport::KernelError;

struct Noop;

impl Service<Bytes> for Noop {
    type Response = Option<Bytes>;
    type Error = KernelError;

    async fn call(&self, _cx: Context, _req: Bytes) -> Result<Self::Response, Self::Error> {
        Ok(None)
    }
}

fn main() -> std::io::Result<()> {
    let spec = HostBuilder::new()
        .use_default_diagnostics()
        .pipeline(|pb| pb.service(Noop))
        .build();

    spark_dist_mio::run_tcp_default(spec)
}
```

控制面默认提供：`/healthz`、`/readyz`、`/metrics`、`/drain`。
