//! Transport contract test suite.
//!
//! 说明：
//! - 该模块只提供“可复用的测试套件函数”，实际测试入口放在 `tests/` 目录。
//! - 设计目标：同一套 suite 以不同 backend factory 运行，保证 channel 语义一致。
//!
//! Rust 最佳实践说明：
//! - 通过 `KeepAlive`（RAII）保持测试资源存活，避免 `mem::forget` 这类泄漏写法。
//! - suite 函数只依赖 Channel 语义对象（`ChannelState`），不直接依赖具体 transport 实现。

use std::any::Any;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use spark_buffer::Bytes;

use spark_transport::async_bridge::contract::ChannelState;
use spark_transport::async_bridge::OutboundFrame;
use spark_transport::evidence::EvidenceSink;
use spark_transport::io::caps;
use spark_transport::reactor::Interest;
use spark_transport::uci::names::evidence as ev_names;
use spark_transport::uci::EvidenceEvent;
use spark_transport::KernelError;

/// Test keep-alive guard.
///
/// 用于保持 TCP/UDP peer socket 等资源在单个 case 内存活。
pub type KeepAlive = Box<dyn Any + Send>;

/// Captures evidence events for assertions.
#[derive(Default)]
pub struct CapturingSink {
    events: Mutex<Vec<EvidenceEvent>>,
}

impl CapturingSink {
    pub fn take(&self) -> Vec<EvidenceEvent> {
        std::mem::take(&mut *self.events.lock().unwrap())
    }
}

impl EvidenceSink for CapturingSink {
    fn emit(&self, event: EvidenceEvent) {
        self.events.lock().unwrap().push(event);
    }
}

fn assert_has_event(events: &[EvidenceEvent], name: &str) {
    assert!(
        events.iter().any(|e| e.name == name),
        "expected evidence event {}, got: {:?}",
        name,
        events
    );
}

/// Runs the “运营可验收” P0 suite: backpressure + draining + close semantics.
///
/// `make_state` should create a fresh `(ChannelState, KeepAlive)` for the backend under test.
pub fn run_p0_suite(
    mut make_state: impl FnMut(Arc<dyn EvidenceSink>) -> (ChannelState, KeepAlive),
) {
    backpressure_enter_exit(&mut make_state);
    draining_close_after_flush(&mut make_state);
    draining_timeout_closes(&mut make_state);
    interest_contract_minimal(&mut make_state);
    close_requested_emits_evidence(&mut make_state);
    peer_half_close_emits_evidence_for_stream(&mut make_state);
    close_is_idempotent_and_closes_io(&mut make_state);
}

fn backpressure_enter_exit(
    make_state: &mut impl FnMut(Arc<dyn EvidenceSink>) -> (ChannelState, KeepAlive),
) {
    let sink = Arc::new(CapturingSink::default());
    // 说明：`make_state` 期望的是 trait object。这里保持 `CapturingSink` 的具体类型，
    // 以便后续断言读取 events；同时通过 Arc 的 unsize coercion 转成 `Arc<dyn EvidenceSink>`。
    let sink_erased: Arc<dyn EvidenceSink> = sink.clone();
    let (mut st, _ka) = make_state(sink_erased);

    // 入队超过 high watermark -> 进入 backpressure（PauseRead + evidence）。
    assert!(st
        .enqueue_outbound(OutboundFrame::from_bytes(Bytes::from_static(
            b"0123456789ABCDEF",
        )))
        .is_ok());
    assert!(
        st.is_read_paused(),
        "expected PauseRead on backpressure enter"
    );

    // 写队列非空 => 必须监听 WRITE（T-01）；读被抑制 => 不应包含 READ。
    let interest = st.desired_interest();
    assert!(
        interest.contains(Interest::WRITE),
        "expected WRITE interest when backlog exists"
    );
    assert!(
        !interest.contains(Interest::READ),
        "expected READ suppressed when paused"
    );

    let evs = sink.take();
    assert_has_event(&evs, ev_names::BACKPRESSURE_ENTER);

    // flush 将写队列清空 -> 退出 backpressure（ResumeRead + evidence）。
    let (_status, _wrote, _syscalls, _writev_calls) = st.flush_outbound();
    assert!(
        !st.is_read_paused(),
        "expected ResumeRead on backpressure exit"
    );
    let interest = st.desired_interest();
    assert!(
        interest.contains(Interest::READ),
        "expected READ interest after resume"
    );

    let evs = sink.take();
    assert_has_event(&evs, ev_names::BACKPRESSURE_EXIT);
}

fn draining_close_after_flush(
    make_state: &mut impl FnMut(Arc<dyn EvidenceSink>) -> (ChannelState, KeepAlive),
) {
    let sink = Arc::new(CapturingSink::default());
    let sink_erased: Arc<dyn EvidenceSink> = sink.clone();
    let (mut st, _ka) = make_state(sink_erased);

    assert!(st
        .enqueue_outbound(OutboundFrame::from_bytes(Bytes::from_static(b"hello")))
        .is_ok());
    st.enter_draining(true, Duration::from_millis(50), 1);
    assert!(st.is_draining());
    assert!(st.is_read_paused(), "draining must PauseRead");

    let evs = sink.take();
    assert_has_event(&evs, ev_names::DRAINING_ENTER);

    // flush 完成后，poll_draining 应触发 close。
    let _ = st.flush_outbound();
    st.poll_draining(0);
    assert!(
        st.is_close_requested(),
        "expected CloseAfterFlush to request close"
    );

    let evs = sink.take();
    assert_has_event(&evs, ev_names::DRAINING_EXIT);
}

fn draining_timeout_closes(
    make_state: &mut impl FnMut(Arc<dyn EvidenceSink>) -> (ChannelState, KeepAlive),
) {
    let sink = Arc::new(CapturingSink::default());
    let sink_erased: Arc<dyn EvidenceSink> = sink.clone();
    let (mut st, _ka) = make_state(sink_erased);

    // 进入 draining 后停止读，允许写 flush 前进；超过 deadline 必须强制关闭并出证据。
    assert!(st
        .enqueue_outbound(OutboundFrame::from_bytes(Bytes::from_static(b"hello")))
        .is_ok());
    st.enter_draining(true, Duration::from_millis(10), 7);
    assert!(st.is_draining());

    // 等待超过超时窗口。
    std::thread::sleep(Duration::from_millis(25));
    st.poll_draining(7);

    assert!(
        st.is_close_requested(),
        "expected draining timeout to request close"
    );
    let evs = sink.take();
    assert_has_event(&evs, ev_names::DRAINING_TIMEOUT);
}

fn interest_contract_minimal(
    make_state: &mut impl FnMut(Arc<dyn EvidenceSink>) -> (ChannelState, KeepAlive),
) {
    let sink = Arc::new(CapturingSink::default());
    let sink_erased: Arc<dyn EvidenceSink> = sink.clone();
    let (mut st, _ka) = make_state(sink_erased);

    // 默认：无 backlog => 只监听 READ。
    let i = st.desired_interest();
    assert!(i.contains(Interest::READ), "expected READ by default");
    assert!(
        !i.contains(Interest::WRITE),
        "expected no WRITE without backlog"
    );

    // draining 会 PauseRead；若无写 backlog，则 interest 必须为空，避免 busy loop（T-01）。
    st.enter_draining(false, Duration::from_millis(50), 0);
    let i = st.desired_interest();
    assert!(
        i.is_empty(),
        "expected empty interest when read paused and no backlog"
    );
    let _ = sink.take();
}

fn close_is_idempotent_and_closes_io(
    make_state: &mut impl FnMut(Arc<dyn EvidenceSink>) -> (ChannelState, KeepAlive),
) {
    let sink = Arc::new(CapturingSink::default());
    let sink_erased: Arc<dyn EvidenceSink> = sink.clone();
    let (mut st, _ka) = make_state(sink_erased);

    st.request_close();
    st.request_close(); // idempotent
    assert!(st.is_close_requested());

    // close 后写必须返回 Closed-ish（T-00 contract）。
    let r = st.io_mut().try_write(b"hi");
    assert!(
        matches!(
            r,
            Err(KernelError::Closed)
                | Err(KernelError::Eof)
                | Err(KernelError::Reset)
                | Err(KernelError::WouldBlock)
                | Err(KernelError::Timeout)
        ),
        "expected closed-ish error after close, got: {:?}",
        r
    );

    let _ = sink.take();
}

fn close_requested_emits_evidence(
    make_state: &mut impl FnMut(Arc<dyn EvidenceSink>) -> (ChannelState, KeepAlive),
) {
    let sink = Arc::new(CapturingSink::default());
    let sink_erased: Arc<dyn EvidenceSink> = sink.clone();
    let (mut st, _ka) = make_state(sink_erased);

    st.request_close();

    let evs = sink.take();
    assert_has_event(&evs, ev_names::CLOSE_REQUESTED);
}

fn peer_half_close_emits_evidence_for_stream(
    make_state: &mut impl FnMut(Arc<dyn EvidenceSink>) -> (ChannelState, KeepAlive),
) {
    let sink = Arc::new(CapturingSink::default());
    let sink_erased: Arc<dyn EvidenceSink> = sink.clone();
    let (mut st, _ka) = make_state(sink_erased);

    // Only meaningful for stream channels.
    if st.io_mut().capabilities() & caps::STREAM == 0 {
        return;
    }

    st.mark_peer_eof();
    assert!(st.is_read_paused(), "peer EOF must PauseRead");

    let i = st.desired_interest();
    assert!(
        !i.contains(Interest::READ),
        "peer EOF must suppress READ interest"
    );

    let evs = sink.take();
    assert_has_event(&evs, ev_names::PEER_HALF_CLOSE);
}
