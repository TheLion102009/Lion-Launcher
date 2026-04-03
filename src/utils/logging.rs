#![allow(dead_code)]

use std::collections::VecDeque;
use std::io::Write;
use std::sync::{Mutex, OnceLock};
use tracing_subscriber::fmt::writer::MakeWriter;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

const MAX_LIVE_LOG_LINES: usize = 6000;

static LIVE_LOG_BUFFER: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();

fn live_log_buffer() -> &'static Mutex<VecDeque<String>> {
    LIVE_LOG_BUFFER.get_or_init(|| Mutex::new(VecDeque::new()))
}

fn push_live_log_line(line: impl Into<String>) {
    if let Ok(mut buf) = live_log_buffer().lock() {
        buf.push_back(line.into());
        while buf.len() > MAX_LIVE_LOG_LINES {
            buf.pop_front();
        }
    }
}

pub fn get_recent_live_logs(limit: usize) -> String {
    let requested = limit.max(1).min(MAX_LIVE_LOG_LINES);
    if let Ok(buf) = live_log_buffer().lock() {
        let start = buf.len().saturating_sub(requested);
        return buf
            .iter()
            .skip(start)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
    }
    String::new()
}

#[derive(Clone, Copy, Default)]
struct TeeWriterFactory;

#[derive(Default)]
struct TeeWriter {
    pending: Vec<u8>,
}

impl Write for TeeWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        std::io::stdout().write_all(buf)?;
        self.pending.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        std::io::stdout().flush()
    }
}

impl Drop for TeeWriter {
    fn drop(&mut self) {
        if self.pending.is_empty() {
            return;
        }

        let text = String::from_utf8_lossy(&self.pending);
        for line in text.lines() {
            if !line.trim().is_empty() {
                push_live_log_line(line.to_string());
            }
        }
        self.pending.clear();
    }
}

impl<'a> MakeWriter<'a> for TeeWriterFactory {
    type Writer = TeeWriter;

    fn make_writer(&'a self) -> Self::Writer {
        TeeWriter::default()
    }
}

pub fn init_logging() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_writer(TeeWriterFactory))
        .try_init();
}
