//! Progress reporting for host applications.
//!
//! With `--progress json` every line on stderr is one JSON object with an `"event"` field:
//!
//! | event | fields |
//! |---|---|
//! | `start` | `scene`, `width`, `height`, `fps`, `renderer`, `output` |
//! | `renderer` | `renderer`, `kind` (`cpu`/`gpu`), `cpu_ms_per_frame`, `gpu_ms_per_frame` (auto calibration settled) |
//! | `warning` | `message` |
//! | `play` | `index`, `name`, `start`, `run_time`, `frames` |
//! | `wait` | `start`, `run_time`, `frames` |
//! | `progress` | `index`, `percent`, `time` (inside a play) |
//! | `marker` | `name`, `time`, `kind` (`marker` or `section`) |
//! | `sound` | `source`, `start`, `gain` |
//! | `problem` | `kind`, `time`, `play`, `a`, `b`, `message` (from `--check`) |
//! | `snapshot` | `path`, `time`, `label` |
//! | `done` | `scene`, `duration`, `frames`, `rasterized`, `elapsed`, `outputs`, `problems`, `renderer` (final backend) |
//! | `error` | `scene`, `message` |
//!
//! `human` (the default) prints readable lines instead and `none` stays quiet.

use std::io::Write;

use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ProgressMode {
    #[default]
    Human,
    Json,
    None,
}

impl std::str::FromStr for ProgressMode {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> anyhow::Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "human" | "text" => Ok(Self::Human),
            "json" | "jsonl" => Ok(Self::Json),
            "none" | "off" => Ok(Self::None),
            _ => anyhow::bail!("unknown progress mode '{s}' (expected human, json or none)"),
        }
    }
}

/// Writes events in the configured mode.
#[derive(Clone, Copy, Debug)]
pub struct Events {
    pub mode: ProgressMode,
}

impl Events {
    pub fn new(mode: ProgressMode) -> Self {
        Events { mode }
    }

    pub fn json(&self) -> bool {
        self.mode == ProgressMode::Json
    }

    pub fn human(&self) -> bool {
        self.mode == ProgressMode::Human
    }

    /// Emit `event` with `fields` (a JSON object) in JSON mode; `human` is printed in human mode.
    pub fn emit(&self, event: &str, fields: Value, human: impl FnOnce() -> Option<String>) {
        match self.mode {
            ProgressMode::Json => {
                let mut obj = match fields {
                    Value::Object(m) => m,
                    _ => serde_json::Map::new(),
                };
                obj.insert("event".into(), json!(event));
                let line = Value::Object(obj).to_string();
                let mut err = std::io::stderr().lock();
                let _ = writeln!(err, "{line}");
            }
            ProgressMode::Human => {
                if let Some(text) = human() {
                    eprintln!("{text}");
                }
            }
            ProgressMode::None => {}
        }
    }
}
