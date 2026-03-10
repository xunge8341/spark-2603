//! HTTP/1 request framing for the management plane.
//!
//! This module is intentionally self-contained so the main stream framing handler stays readable.
//! The goal is to keep borrow scopes small and explicit (auditability), while preserving a
//! predictable buffering policy and accurate inbound copy metrics.

use std::collections::VecDeque;

use spark_buffer::{Bytes, BytesMut, Cumulation};
use spark_core::context::Context as DecodeContext;

use spark_codec::{ByteToMessageDecoder, DecodeOutcome};
use spark_codec_http::http1::{Http1DecodeError, Http1HeadDecoder};

/// Error surfaced by the HTTP/1 append/decode loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Http1AppendError {
    TooLarge { size: usize, max: usize },
    Decode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct Http1DecodeBatchStats {
    pub produced: usize,
    pub coalesce_count: usize,
    pub copied_bytes: usize,
}

#[derive(Debug, Clone)]
struct PendingHttp1 {
    head_consumed: usize,
    content_len: usize,
}

/// Minimal HTTP/1 inbound state: decode head, then wait for the body.
///
/// Buffering policy:
/// - `max_request_bytes` bounds a single request (head + body).
/// - `max_buffered` bounds total buffered bytes to avoid unbounded pipelining.
///   This still allows a request to complete even if extra bytes for the next request have already arrived.
#[derive(Debug)]
pub(super) struct Http1InboundState {
    max_request_bytes: usize,
    max_head_bytes: usize,
    max_buffered: usize,

    decoder: Http1HeadDecoder,
    pending: Option<PendingHttp1>,

    cumulation: Cumulation,
    scratch: BytesMut,
    ready: VecDeque<Bytes>,
}

impl Http1InboundState {
    pub(super) fn new(max_request_bytes: usize, max_head_bytes: usize, max_headers: usize) -> Self {
        let max_request_bytes = max_request_bytes.max(1);
        let max_head_bytes = max_head_bytes.max(1).min(max_request_bytes);
        let max_headers = max_headers.max(1);

        // Allow a request to complete even if extra bytes have already arrived,
        // but cap total buffering to avoid unbounded pipelining.
        let max_buffered = max_request_bytes.saturating_add(max_head_bytes);

        Self {
            max_request_bytes,
            max_head_bytes,
            max_buffered,
            decoder: Http1HeadDecoder::with_limits(max_head_bytes, max_headers),
            pending: None,
            cumulation: Cumulation::with_capacity(max_request_bytes.min(64 * 1024)),
            scratch: BytesMut::with_capacity(max_head_bytes.min(64 * 1024)),
            ready: VecDeque::new(),
        }
    }

    #[inline]
    pub(super) fn pop_ready(&mut self) -> Option<Bytes> {
        self.ready.pop_front()
    }

    pub(super) fn append_stream_bytes(
        &mut self,
        ctx: &mut DecodeContext,
        bytes: &[u8],
    ) -> core::result::Result<Http1DecodeBatchStats, Http1AppendError> {
        self.cumulation.push_bytes(bytes);
        if self.cumulation.len() > self.max_buffered {
            return Err(Http1AppendError::TooLarge {
                size: self.cumulation.len(),
                max: self.max_buffered,
            });
        }

        let mut stats = Http1DecodeBatchStats::default();

        // Bound per-call work to avoid starving other connections.
        const MAX_DECODE_PER_CALL: usize = 8;

        for _ in 0..MAX_DECODE_PER_CALL {
            if self.cumulation.is_empty() {
                break;
            }

            if self.pending.is_none() {
                let live = self.cumulation.len();
                let prefix_len = live.min(self.max_head_bytes);

                // Borrow hygiene: the head slice may reference `scratch`. Move it out temporarily,
                // run the decoder, then restore it so borrows stay non-overlapping.
                let mut scratch = core::mem::take(&mut self.scratch);
                let head = head_prefix_from(&self.cumulation, &mut scratch, prefix_len);
                let outcome = self.decoder.decode(ctx, head);
                self.scratch = scratch;

                match outcome {
                    Ok(DecodeOutcome::NeedMore) => {
                        // If we already have max_head_bytes and still cannot complete the head,
                        // classify it as too large.
                        if live >= self.max_head_bytes {
                            return Err(Http1AppendError::TooLarge {
                                size: self.max_head_bytes.saturating_add(1),
                                max: self.max_head_bytes,
                            });
                        }
                        break;
                    }
                    Ok(DecodeOutcome::Message { consumed, message }) => {
                        let content_len = message.content_length();
                        let total = consumed.saturating_add(content_len);
                        if total > self.max_request_bytes {
                            return Err(Http1AppendError::TooLarge {
                                size: total,
                                max: self.max_request_bytes,
                            });
                        }
                        self.pending = Some(PendingHttp1 {
                            head_consumed: consumed,
                            content_len,
                        });
                    }
                    Err(Http1DecodeError::HeadTooLarge) => {
                        return Err(Http1AppendError::TooLarge {
                            size: self.max_head_bytes.saturating_add(1),
                            max: self.max_head_bytes,
                        });
                    }
                    Err(_) => return Err(Http1AppendError::Decode),
                }
            }

            let Some(p) = self.pending.as_ref() else {
                break;
            };
            let total = p.head_consumed.saturating_add(p.content_len);
            if self.cumulation.len() < total {
                break;
            }

            // Emit the full request bytes (head + body).
            let (req, ts) = self.cumulation.take_message_with_stats(total, total);
            if ts.coalesced {
                stats.coalesce_count = stats.coalesce_count.saturating_add(1);
                stats.copied_bytes = stats.copied_bytes.saturating_add(ts.copied_bytes);
            }

            if !req.is_empty() {
                self.ready.push_back(req);
                stats.produced = stats.produced.saturating_add(1);
            }

            self.pending = None;
        }

        Ok(stats)
    }
}

fn head_prefix_from<'a>(cumulation: &'a Cumulation, scratch: &'a mut BytesMut, want: usize) -> &'a [u8] {
    if want == 0 {
        return &[];
    }

    // Fast-path: a single segment (common thanks to Cumulation's tail buffer).
    let mut it = cumulation.iter_segments();
    let Some(first) = it.next() else {
        return &[];
    };
    if first.len() >= want {
        return &first[..want];
    }

    // Multi-segment: coalesce only the head prefix into a scratch buffer for the head decoder.
    scratch.clear();
    scratch.reserve(want);
    let mut rem = want;
    for seg in cumulation.iter_segments() {
        if rem == 0 {
            break;
        }
        let n = seg.len().min(rem);
        scratch.extend_from_slice(&seg[..n]);
        rem -= n;
    }
    debug_assert_eq!(scratch.len(), want);
    scratch.as_slice()
}
