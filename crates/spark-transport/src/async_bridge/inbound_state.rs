use std::collections::VecDeque;

use spark_buffer::{Bytes, Cumulation, TakeStats};

use spark_core::context::Context;

use crate::{KernelError, Result};

use super::framers::{
    DelimiterFramer, FrameSpec, LengthFieldPrefixFramer, LineFramer, StreamDecodeError,
    StreamFramer, Varint32Framer,
};
use super::pipeline::DelimiterSpec;

/// Stats for a single `append_stream_bytes` decode batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DecodeBatchStats {
    pub produced: usize,
    pub coalesce_count: usize,
    pub copied_bytes: usize,
    /// Bytes copied when appending into cumulation tail (`push_bytes`).
    pub cumulation_copy_bytes: usize,
}

/// Per-connection inbound (read-path) state.
///
/// This is a minimal Netty/DotNetty-style inbound model:
/// - Maintain a segmented stream cumulation.
/// - On `readable`, run a bounded decode loop to emit as many messages as possible.
/// - Queue decoded messages for service consumption.
#[derive(Debug)]
pub struct InboundState {
    // Stream state.
    cumulation: Cumulation,
    framer: StreamFramer,

    // Ready-to-dispatch decoded messages.
    ready: VecDeque<Bytes>,
}

impl InboundState {
    pub fn new_line(max_frame: usize) -> Self {
        Self {
            cumulation: Cumulation::with_capacity(max_frame.min(64 * 1024)),
            // bring-up default: include delimiter bytes (same as previous behavior).
            framer: StreamFramer::Line(LineFramer::new(max_frame.max(1), true)),
            ready: VecDeque::new(),
        }
    }

    pub fn new_delimiter(
        max_frame: usize,
        delimiter: DelimiterSpec,
        include_delimiter: bool,
    ) -> Self {
        Self {
            cumulation: Cumulation::with_capacity(max_frame.min(64 * 1024)),
            framer: StreamFramer::Delimiter(DelimiterFramer::new(
                max_frame.max(1),
                delimiter,
                include_delimiter,
            )),
            ready: VecDeque::new(),
        }
    }

    pub fn new_length_field(
        max_frame: usize,
        field_len: usize,
        order: spark_codec::ByteOrder,
    ) -> Option<Self> {
        let framer = LengthFieldPrefixFramer::new(max_frame.max(1), field_len, order)?;
        Some(Self {
            cumulation: Cumulation::with_capacity(max_frame.min(64 * 1024)),
            framer: StreamFramer::LengthField(framer),
            ready: VecDeque::new(),
        })
    }

    pub fn new_varint32(max_frame: usize) -> Self {
        Self {
            cumulation: Cumulation::with_capacity(max_frame.min(64 * 1024)),
            framer: StreamFramer::Varint32(Varint32Framer::new(max_frame.max(1))),
            ready: VecDeque::new(),
        }
    }

    #[inline]
    #[allow(dead_code)]
    pub fn has_ready(&self) -> bool {
        !self.ready.is_empty()
    }

    #[inline]
    pub fn pop_ready(&mut self) -> Option<Bytes> {
        self.ready.pop_front()
    }

    /// Current buffered stream bytes (cumulation length).
    #[inline]
    pub fn cumulation_len(&self) -> usize {
        self.cumulation.len()
    }

    /// For datagram transports (UDP), bypass stream cumulation and queue the payload directly.
    #[allow(dead_code)]
    pub fn push_datagram(&mut self, msg: Bytes) {
        if !msg.is_empty() {
            self.ready.push_back(msg);
        }
    }

    /// Append newly read stream bytes and decode as many complete messages as possible.
    ///
    /// Boundary contract:
    /// - `bytes` is consumed synchronously in this call and copied into cumulation tail.
    /// - Borrowed input must not be retained beyond this function.
    ///
    /// Decoded messages are exposed as `Bytes`.
    pub fn append_stream_bytes(
        &mut self,
        ctx: &mut Context,
        bytes: &[u8],
    ) -> Result<DecodeBatchStats> {
        let _ = ctx;
        self.cumulation.push_bytes(bytes);

        let mut stats = DecodeBatchStats {
            cumulation_copy_bytes: bytes.len(),
            ..DecodeBatchStats::default()
        };

        // Bound per-call work to avoid starving other connections.
        const MAX_DECODE_PER_CALL: usize = 32;

        for _ in 0..MAX_DECODE_PER_CALL {
            if self.cumulation.is_empty() {
                break;
            }

            let Some(spec) = self
                .framer
                .decode(&self.cumulation)
                .map_err(map_decode_err)?
            else {
                break;
            };

            let (m, ts) = take_message(&mut self.cumulation, spec);
            accumulate_take_stats(&mut stats, ts);

            if !m.is_empty() {
                self.ready.push_back(m);
                stats.produced += 1;
            }
        }

        Ok(stats)
    }
}

#[inline]
fn take_message(c: &mut Cumulation, spec: FrameSpec) -> (Bytes, TakeStats) {
    if spec.msg_start == 0 {
        c.take_message_with_stats(spec.consumed, spec.msg_end)
    } else {
        c.take_range_with_stats(spec.consumed, spec.msg_start, spec.msg_end)
    }
}

#[inline]
fn accumulate_take_stats(stats: &mut DecodeBatchStats, ts: TakeStats) {
    if ts.coalesced {
        stats.coalesce_count = stats.coalesce_count.saturating_add(1);
        stats.copied_bytes = stats.copied_bytes.saturating_add(ts.copied_bytes);
    }
}

fn map_decode_err(e: StreamDecodeError) -> KernelError {
    match e {
        StreamDecodeError::FrameTooLong => KernelError::Invalid,
        StreamDecodeError::InvalidDelimiter => KernelError::Invalid,
        StreamDecodeError::CorruptedLengthField => KernelError::Invalid,
    }
}
