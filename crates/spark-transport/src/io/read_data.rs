use spark_uci::RxToken;

/// How the read bytes are delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadData {
    /// Bytes were copied into the caller-provided buffer.
    Copied,
    /// Bytes are referenced by a zero-copy RX token.
    Token(RxToken),
}
