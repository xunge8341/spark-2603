use spark_transport::async_bridge::contract::{FlushStatus, OutboundBuffer};
use spark_transport::policy::FlushBudget;
use spark_transport::io::{ChannelCaps, IoOps, ReadOutcome, Result, RxToken};
use spark_transport::KernelError;
use spark_buffer::Bytes;

/// Contract: outbound buffer must preserve byte stream ordering across partial vectored writes.
///
/// DECISION (Multi-backend correctness): different backends (epoll/kqueue/IOCP/WASI) may report
/// partial progress on `writev`/gather sends. The transport core must be able to make forward
/// progress without dropping/duplicating bytes, *independent* of reactor event semantics.
#[test]
fn outbound_buffer_survives_partial_vectored_writes() {
    // Build a small buffer with two frames so flush uses vectored writes.
    let mut ob = OutboundBuffer::new(1024 * 1024, 0);

    let f1 = spark_transport::async_bridge::OutboundFrame::from_bytes(Bytes::from_static(b"hello "));
    let f2 = spark_transport::async_bridge::OutboundFrame::from_bytes(Bytes::from_static(b"world"))
        .append_suffix(b"\n");
    ob.enqueue(f1);
    ob.enqueue(f2);

    let mut io = MockIo::new(3); // only 3 bytes per syscall to force partial writes
    // DECISION: avoid cross-crate struct literals (FlushBudget is non-exhaustive).
    let budget = FlushBudget::new(1024 * 1024, 1024).with_max_iov(16);

    let (st, written, syscalls, writev_calls, _wc) = ob.flush_into(&mut io, budget);
    assert_eq!(st, FlushStatus::Drained);
    assert!(syscalls > 1);
    assert!(writev_calls > 0);
    assert_eq!(written, b"hello world\n".len());
    assert_eq!(io.sink, b"hello world\n");
}

#[derive(Debug)]
struct MockIo {
    max_per_call: usize,
    sink: Vec<u8>,
}

impl MockIo {
    fn new(max_per_call: usize) -> Self {
        Self { max_per_call: max_per_call.max(1), sink: Vec::new() }
    }
}

impl IoOps for MockIo {
    fn capabilities(&self) -> ChannelCaps {
        spark_transport::io::STREAM
    }

    fn try_read_lease(&mut self) -> Result<ReadOutcome> {
        Err(KernelError::Unsupported)
    }

    fn try_read_into(&mut self, _dst: &mut [u8]) -> Result<ReadOutcome> {
        Err(KernelError::WouldBlock)
    }

    fn try_write(&mut self, data: &[u8]) -> Result<usize> {
        if data.is_empty() {
            return Ok(0);
        }
        let n = data.len().min(self.max_per_call);
        self.sink.extend_from_slice(&data[..n]);
        Ok(n)
    }

    fn try_write_vectored(&mut self, bufs: &[&[u8]]) -> Result<usize> {
        let mut remaining = self.max_per_call;
        let mut written = 0usize;
        for b in bufs {
            if b.is_empty() {
                continue;
            }
            if remaining == 0 {
                break;
            }
            let take = b.len().min(remaining);
            self.sink.extend_from_slice(&b[..take]);
            written += take;
            remaining -= take;
            if take < b.len() {
                // partial within this segment
                break;
            }
        }
        Ok(written)
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }

    fn rx_ptr_len(&mut self, _tok: RxToken) -> Option<(*const u8, usize)> {
        None
    }

    fn release_rx(&mut self, _tok: RxToken) {}
}
