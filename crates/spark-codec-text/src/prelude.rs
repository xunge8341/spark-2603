//! Prelude for Spark text codecs.
//!
//! Importing this module brings the core framing/headers types **and** the common
//! ASCII/token helpers into scope.

pub use crate::{
    ascii_box_str, ascii_lower_box_str, is_token, split_ws_token, trim_ows, CrlfHeadDecoder,
    AsciiValueConverter, DefaultHeaders, HeaderEntry, HeaderLine, HeaderMap, HeaderName, HeaderValue,
    TextHead, TextHeadError, TextHeadLimits, ValueConvertError, ValueConverter,
};
