#[derive(Debug, Clone)]
pub struct MgmtResponse {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

impl MgmtResponse {
    pub fn ok(body: impl Into<Vec<u8>>) -> Self {
        Self { status: 200, content_type: "text/plain; charset=utf-8", body: body.into() }
    }

    pub fn status(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self { status, content_type: "text/plain; charset=utf-8", body: body.into() }
    }
}
