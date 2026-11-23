use super::super::client::api_error;
use crate::remote::http;
use crate::Result;

#[derive(Default)]
pub(super) struct Parser {
    line: Vec<u8>,
    event: String,
    data: String,
    size: u64,
}

impl Parser {
    pub(super) fn push(
        &mut self,
        byte: u8,
        limit: u64,
    ) -> Result<Option<(String, serde_json::Value)>> {
        self.size = self.size.saturating_add(1);
        http::check_size(self.size, limit, "langsmith", "SSE event bytes")?;
        if byte != b'\n' {
            self.line.push(byte);
            return Ok(None);
        }
        let line = std::str::from_utf8(&self.line)
            .map_err(|_| api_error("invalid SSE encoding"))?
            .trim_end_matches('\r');
        if line.is_empty() {
            self.size = 0;
            self.line.clear();
            if self.event.is_empty() && self.data.is_empty() {
                return Ok(None);
            }
            let value =
                serde_json::from_str(&self.data).map_err(|_| api_error("invalid SSE JSON"))?;
            self.data.clear();
            return Ok(Some((std::mem::take(&mut self.event), value)));
        }
        if let Some((field, value)) = line.split_once(':') {
            let value = value.strip_prefix(' ').unwrap_or(value);
            match field {
                "event" => self.event = value.into(),
                "data" => {
                    if !self.data.is_empty() {
                        self.data.push('\n');
                    }
                    self.data.push_str(value);
                }
                _ => {}
            }
        }
        self.line.clear();
        Ok(None)
    }
}
