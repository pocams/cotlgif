use std::io;
use std::str::FromStr;

use axum::http::StatusCode;
use axum::Json;
use axum::response::{IntoResponse, Response};
use color_eyre::Report;
use once_cell::sync::OnceCell;
use regex::Regex;
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

#[derive(Debug, Copy, Clone)]
pub enum OutputType {
    Gif,
    Apng,
    Png,
    Mp4,
}

impl OutputType {
    pub fn mime_type(&self) -> &'static str {
        match self {
            OutputType::Gif => "image/gif",
            OutputType::Apng | OutputType::Png => "image/png",
            OutputType::Mp4 => "video/mp4",
        }
    }

    pub fn extension(&self) -> &'static str {
        match self {
            OutputType::Gif => "gif",
            OutputType::Apng | OutputType::Png => "png",
            OutputType::Mp4 => "mp4",
        }
    }
}

impl Default for OutputType {
    fn default() -> Self {
        OutputType::Apng
    }
}

impl FromStr for OutputType {
    type Err = JsonError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_ref() {
            "gif" => Ok(OutputType::Gif),
            "png" => Ok(OutputType::Png),
            "apng"  => Ok(OutputType::Apng),
            "mp4" => Ok(OutputType::Mp4),
            _ => Err(json_400("Invalid format, expected gif, png, apng, mp4"))
        }
    }
}
