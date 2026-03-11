use std::sync::Arc;

use spark_buffer::Bytes;

use super::super::dyn_channel::DynChannel;
use crate::evidence::EvidenceSink;
use crate::KernelError;
use spark_core::service::Service;

use super::handler::ChannelHandler;
use super::{AppServiceOptions, ChannelPipeline, HandlerVec};
use super::{DelimiterSpec, FrameDecoderProfile};

/// Options for line-based framing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineOptions {
    pub max_frame: usize,
}

impl LineOptions {
    #[inline]
    pub fn new(max_frame: usize) -> Self {
        Self {
            max_frame: max_frame.max(1),
        }
    }
}

/// Options for delimiter-based framing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DelimiterOptions {
    pub max_frame: usize,
    pub delimiter: DelimiterSpec,
    pub include_delimiter: bool,
}

impl DelimiterOptions {
    #[inline]
    pub fn new(max_frame: usize, delimiter: DelimiterSpec, include_delimiter: bool) -> Self {
        Self {
            max_frame: max_frame.max(1),
            delimiter,
            include_delimiter,
        }
    }
}

/// Options for HTTP/1 management-plane framing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Http1Options {
    pub max_request_bytes: usize,
    pub max_head_bytes: usize,
    pub max_headers: usize,
}

impl Http1Options {
    #[inline]
    pub fn with_limits(
        max_request_bytes: usize,
        max_head_bytes: usize,
        max_headers: usize,
    ) -> Self {
        let max_request_bytes = max_request_bytes.max(1);
        let max_head_bytes = max_head_bytes.max(1).min(max_request_bytes);
        let max_headers = max_headers.max(1);
        Self {
            max_request_bytes,
            max_head_bytes,
            max_headers,
        }
    }
}

/// Options for length-field prefixed framing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LengthFieldOptions {
    pub max_frame: usize,
    pub field_len: usize,
    pub order: spark_codec::ByteOrder,
}

impl LengthFieldOptions {
    #[inline]
    pub fn new(max_frame: usize, field_len: usize, order: spark_codec::ByteOrder) -> Option<Self> {
        match field_len {
            1 | 2 | 3 | 4 | 8 => {}
            _ => return None,
        }
        Some(Self {
            max_frame: max_frame.max(1),
            field_len,
            order,
        })
    }
}

/// Options for varint32 length-prefixed framing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Varint32Options {
    pub max_frame: usize,
}

impl Varint32Options {
    #[inline]
    pub fn new(max_frame: usize) -> Self {
        Self {
            max_frame: max_frame.max(1),
        }
    }
}

/// ChannelPipeline 构建器（bring-up 版本）。
///
/// 设计动机（DotNetty 经验）：
/// - Pipeline 的“语义位置”要先立住（inbound/outbound 方向、事件传播），
///   具体 handler 的组合再逐步开放。
/// - bring-up 阶段先提供一组稳定的默认 handler 链：
///   Head -> FrameDecoder -> AppService -> Tail。
/// - 后续要做到 Netty/DotNetty 同级体验，可以在此基础上扩展 add_last/add_first。
pub struct ChannelPipelineBuilder<A, Ev = Arc<dyn EvidenceSink>, Io = Box<dyn DynChannel>> {
    app: Arc<A>,
    profile: FrameDecoderProfile,

    /// 额外的 handler：插入到 Head 之后（更靠近 head）。
    ///
    /// Netty/DotNetty 语义：addFirst。
    first: HandlerVec<A, Ev, Io>,

    /// 额外的 handler：插入到 Tail 之前（更靠近 tail）。
    ///
    /// Netty/DotNetty 语义：addLast。
    last: HandlerVec<A, Ev, Io>,
    app_service: AppServiceOptions,
}

impl<A, Ev, Io> ChannelPipelineBuilder<A, Ev, Io>
where
    A: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
    Ev: EvidenceSink,
    Io: DynChannel,
{
    /// 创建 builder。
    pub fn new(app: Arc<A>) -> Self {
        Self {
            app,
            // bring-up default: line-based framing.
            profile: FrameDecoderProfile::line(64 * 1024),

            first: Vec::new(),
            last: Vec::new(),
            app_service: AppServiceOptions::default(),
        }
    }

    /// Use line-based framing (default).
    pub fn line(self, opts: LineOptions) -> Self {
        self.framing(FrameDecoderProfile::line(opts.max_frame))
    }

    /// Use delimiter-based framing.
    pub fn delimiter(self, opts: DelimiterOptions) -> Self {
        self.framing(FrameDecoderProfile::Delimiter {
            max_frame: opts.max_frame,
            delimiter: opts.delimiter,
            include_delimiter: opts.include_delimiter,
        })
    }

    /// Use HTTP/1 request framing (management-plane).
    pub fn http1(self, opts: Http1Options) -> Self {
        self.framing(FrameDecoderProfile::http1_with_limits(
            opts.max_request_bytes,
            opts.max_head_bytes,
            opts.max_headers,
        ))
    }

    /// Use length-field prefixed framing.
    pub fn length_field(self, opts: LengthFieldOptions) -> Self {
        // `LengthFieldOptions` construction already validates the field length.
        self.framing(FrameDecoderProfile::LengthField {
            max_frame: opts.max_frame,
            field_len: opts.field_len as u8,
            order: opts.order,
        })
    }

    /// Use varint32 length-prefixed framing.
    pub fn varint32(self, opts: Varint32Options) -> Self {
        self.framing(FrameDecoderProfile::varint32(opts.max_frame))
    }

    /// Set the exact framing profile for the built-in stream decoder.
    pub fn framing(mut self, profile: FrameDecoderProfile) -> Self {
        self.profile = profile;
        self
    }

    /// 在 pipeline 头部插入一个 handler（Head 之后）。
    ///
    /// 说明：
    /// - inbound：该 handler 会较早看到事件（更靠近 head）。
    /// - outbound：从业务 handler 调用 `ctx.write/flush` 时，会经过 head 方向的 handler；
    ///   因此 encoder 类 handler 通常适合放在较靠近 head 的位置。
    ///   插入到 head 之后（语义借鉴 Netty/DotNetty `addFirst`）。
    #[allow(dead_code)]
    pub fn add_first<H>(mut self, h: H) -> Self
    where
        H: ChannelHandler<A, Ev, Io> + 'static,
    {
        self.first.push(Box::new(h));
        self
    }

    /// 在 pipeline 尾部插入一个 handler（Tail 之前）。
    ///
    /// 说明：
    /// - inbound：该 handler 会较晚看到事件（更靠近 tail）。
    /// - outbound：若调用的是 `channel.write`（从 tail 开始），该 handler 会更早执行。
    ///   插入到 tail 之前（语义借鉴 Netty/DotNetty `addLast`）。
    #[allow(dead_code)]
    pub fn add_last<H>(mut self, h: H) -> Self
    where
        H: ChannelHandler<A, Ev, Io> + 'static,
    {
        self.last.push(Box::new(h));
        self
    }

    /// Configure per-connection app-service concurrency and queue behavior.
    pub fn app_service_options(mut self, opts: AppServiceOptions) -> Self {
        self.app_service = opts.normalized();
        self
    }

    /// 生成 pipeline。
    pub fn build(self) -> ChannelPipeline<A, Ev, Io> {
        ChannelPipeline::new_with_extras(
            self.app,
            self.profile,
            self.first,
            self.last,
            self.app_service,
        )
    }
}
