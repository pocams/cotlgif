use std::io;
use axum::http::StatusCode;
use color_eyre::Report;
use once_cell::sync::OnceCell;
use regex::Regex;
use axum::Json;
use axum::response::{IntoResponse, Response};
use serde_json::json;

pub struct ChannelWriter {
    sender: Option<futures_channel::mpsc::UnboundedSender<Result<Vec<u8>, Report>>>
}

impl ChannelWriter {
    pub fn new(sender: futures_channel::mpsc::UnboundedSender<Result<Vec<u8>, Report>>) -> ChannelWriter {
        ChannelWriter { sender: Some(sender) }
    }
}

impl io::Write for ChannelWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.sender.as_ref().unwrap().unbounded_send(Ok(buf.to_vec())).map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "Receiver closed"))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        drop(self.sender.take());
        Ok(())
    }
}

pub trait Slug {
    fn slugify_string(s: &str) -> String {
        static NON_ALPHA: OnceCell<Regex> = OnceCell::new();
        static LOWER_UPPER: OnceCell<Regex> = OnceCell::new();
        let non_alpha = NON_ALPHA.get_or_init(|| Regex::new(r"[^A-Za-z0-9]").unwrap());
        let lower_upper = LOWER_UPPER.get_or_init(|| Regex::new(r"([a-z])([A-Z])").unwrap());
        let s = non_alpha.replace(s, "-");
        let s = lower_upper.replace(&s, "$2-$1");
        s.to_ascii_lowercase().to_owned()
    }

    fn slug(&self) -> String;
}

pub struct JsonError {
    message: String,
    status: StatusCode
}

pub fn json_400(s: impl Into<String>) -> JsonError {
    JsonError { message: s.into(), status: StatusCode::BAD_REQUEST }
}

pub fn json_404(s: impl Into<String>) -> JsonError {
    JsonError { message: s.into(), status: StatusCode::NOT_FOUND }
}

impl IntoResponse for JsonError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({"error": self.message}))).into_response()
    }
}
