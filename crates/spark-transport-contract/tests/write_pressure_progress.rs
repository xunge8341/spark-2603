use std::sync::Arc;

use spark_buffer::Bytes;

use spark_transport::async_bridge::contract::{ChannelState, FlushStatus};
use spark_transport::async_bridge::OutboundFrame;
use spark_transport::policy::FlushPolicy;
use spark_transport::reactor::Interest;
use spark_transport::{EvidenceSink, NoopEvidenceSink};

use spark_transport_contract::fake_io::ScriptedIo;

/// 验证：高写压/partial write 下，flush 能推进并在 WouldBlock 时给出 WRITE interest，避免 busy loop.
///
/// 说明：
/// - 该测试不依赖 OS socket，使用脚本化 IO 稳定复现 WouldBlock/partial write；
/// - 验收口径来自 T-01/T-TCP-01：
///   - 无进展时必须退回 WouldBlock；
///   - 写队列非空 => 必须需要 WRITE interest；
///   - writable 后续到来（允许写预算） => flush 能前进直至 Drained。
#[test]
fn flush_progress_and_interest_contract() {
    let sink: Arc<dyn EvidenceSink> = Arc::new(NoopEvidenceSink);

    let flush = FlushPolicy::default().budget(64 * 1024);

    let io = Box::new(ScriptedIo::new());
    let mut st = ChannelState::new(
        1,
        io,
        1_000_000,
        500_000,
        usize::MAX,
        flush,
        Arc::clone(&sink),
    );

    // 入队两个 frame（总量 8KB）。
    assert!(st
        .enqueue_outbound(OutboundFrame::from_bytes(Bytes::from(vec![0u8; 4096])))
        .is_ok());
    assert!(st
        .enqueue_outbound(OutboundFrame::from_bytes(Bytes::from(vec![0u8; 4096])))
        .is_ok());

    // 模拟：当前仅允许写 1024 bytes（部分写后立即 WouldBlock）。
    {
        let io = st
            .io_mut()
            .as_any_mut()
            .downcast_mut::<ScriptedIo>()
            .unwrap();
        io.add_allowance(1024);
    }

    let (st1, wrote1, _syscalls1, _writev1) = st.flush_outbound();
    assert!(wrote1 > 0, "expected some progress before WouldBlock");
    assert_eq!(
        st1,
        FlushStatus::WouldBlock,
        "expected WouldBlock after allowance exhausted"
    );

    // 写队列仍非空 => 必须监听 WRITE，避免 busy loop。
    let interest = st.desired_interest();
    assert!(interest.contains(Interest::WRITE));
    assert!(interest.contains(Interest::READ)); // 未进入 PauseRead

    // 模拟：writable 事件到来，增加写预算，flush 应可最终 drain。
    {
        let io = st
            .io_mut()
            .as_any_mut()
            .downcast_mut::<ScriptedIo>()
            .unwrap();
        io.add_allowance(16 * 1024);
    }

    let (st2, wrote2, _syscalls2, _writev2) = st.flush_outbound();
    assert!(wrote2 > 0);
    assert_eq!(st2, FlushStatus::Drained, "expected Drained");

    // 写调用次数应在合理范围内（防止内部忙等/自旋）。
    let calls = st
        .io_mut()
        .as_any_mut()
        .downcast_mut::<ScriptedIo>()
        .unwrap()
        .write_calls;
    assert!(calls <= 32, "unexpectedly high write call count: {}", calls);
}
