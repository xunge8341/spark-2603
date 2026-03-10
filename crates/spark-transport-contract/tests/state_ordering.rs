use std::sync::Arc;
use std::time::Duration;

use spark_buffer::Bytes;

use spark_transport::async_bridge::contract::ChannelState;
use spark_transport::async_bridge::OutboundFrame;
use spark_transport::evidence::EvidenceSink;
use spark_transport::policy::FlushPolicy;
use spark_transport::reactor::Interest;
use spark_transport::uci::names::evidence as ev_names;

use spark_transport_contract::fake_io::ScriptedIo;
use spark_transport_contract::suite::CapturingSink;

#[test]
fn writability_change_and_interest_follow_watermarks_in_order() {
    let sink = Arc::new(CapturingSink::default());
    let sink_erased: Arc<dyn EvidenceSink> = sink.clone();
    let io: Box<dyn spark_transport::async_bridge::contract::DynChannel> =
        Box::new(ScriptedIo::new());
    let flush = FlushPolicy::default().budget(64 * 1024);
    let mut st = ChannelState::new(1, io, 8 * 1024, 4 * 1024, usize::MAX, flush, sink_erased);

    // enqueue > high watermark => BackpressureEnter, READ paused, interest WRITE only.
    assert!(st
        .enqueue_outbound(OutboundFrame::from_bytes(Bytes::from(vec![0u8; 16 * 1024])))
        .is_ok());
    let i = st.desired_interest();
    assert!(i.contains(Interest::WRITE));
    assert!(!i.contains(Interest::READ));

    let wc1 = st.take_writability_change();
    assert!(matches!(
        wc1,
        spark_transport::async_bridge::contract::WritabilityChange::BecameUnwritable { .. }
    ));

    // allow writes and flush to drain => BackpressureExit, interest READ.
    {
        let io = st
            .io_mut()
            .as_any_mut()
            .downcast_mut::<ScriptedIo>()
            .expect("scripted io");
        io.add_allowance(64 * 1024);
    }

    let (_st, _wrote, _syscalls, _writev_calls) = st.flush_outbound();
    let wc2 = st.take_writability_change();
    assert!(matches!(
        wc2,
        spark_transport::async_bridge::contract::WritabilityChange::BecameWritable { .. }
    ));

    let i2 = st.desired_interest();
    assert!(i2.contains(Interest::READ));

    let evs = sink.take();
    let names: Vec<&'static str> = evs.iter().map(|e| e.name).collect();
    let enter = names
        .iter()
        .position(|n| *n == ev_names::BACKPRESSURE_ENTER)
        .unwrap();
    let exit = names
        .iter()
        .position(|n| *n == ev_names::BACKPRESSURE_EXIT)
        .unwrap();
    assert!(
        enter < exit,
        "expected BackpressureEnter before BackpressureExit: {:?}",
        names
    );
}

#[test]
fn draining_timeout_emits_in_order_and_requests_close() {
    let sink = Arc::new(CapturingSink::default());
    let sink_erased: Arc<dyn EvidenceSink> = sink.clone();
    let io: Box<dyn spark_transport::async_bridge::contract::DynChannel> =
        Box::new(ScriptedIo::new());
    let flush = FlushPolicy::default().budget(64 * 1024);
    let mut st = ChannelState::new(1, io, 8 * 1024, 4 * 1024, usize::MAX, flush, sink_erased);

    st.enter_draining(false, Duration::from_millis(10), 0);
    assert!(
        st.desired_interest().is_empty(),
        "expected empty interest when draining without backlog"
    );

    std::thread::sleep(Duration::from_millis(20));
    st.poll_draining(0);
    assert!(
        st.is_close_requested(),
        "expected close requested after draining timeout"
    );

    let evs = sink.take();
    let names: Vec<&'static str> = evs.iter().map(|e| e.name).collect();
    let enter = names
        .iter()
        .position(|n| *n == ev_names::DRAINING_ENTER)
        .unwrap();
    let timeout = names
        .iter()
        .position(|n| *n == ev_names::DRAINING_TIMEOUT)
        .unwrap();
    assert!(
        enter < timeout,
        "expected DrainingEnter before DrainingTimeout: {:?}",
        names
    );
}
