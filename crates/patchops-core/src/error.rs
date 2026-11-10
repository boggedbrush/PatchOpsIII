//! Error types.
//!
//! Two levels, mirroring the legacy HTTP API (see `bench/OPS.md` section 1):
//!
//! * [`OpError`]: the JSON-RPC *envelope* failed (`validation:`, `unknown op:`, `internal:`).
//! * [`AppError`]: the op ran but reports an application-level failure, which the legacy API
//!   returned as `{"ok": false, "error": "..."}` (HTTP 200). Typed handlers return
//!   `Result<T, AppError>`; the dispatch layer turns it into that JSON body.

use serde_json::{Map, Value};

use crate::state::State;

/// Envelope-level failure. `Display` gives the exact protocol string (`"<class>: <detail>"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpError {
    /// Invalid arguments (legacy: HTTP 422).
    Validation(String),
    /// No such op.
    UnknownOp(String),
    /// Unexpected failure (legacy: HTTP 500), or an op that is not ported yet.
    Internal(String),
}

impl OpError {
    pub fn validation(msg: impl Into<String>) -> Self {
        OpError::Validation(msg.into())
    }
    pub fn internal(msg: impl Into<String>) -> Self {
        OpError::Internal(msg.into())
    }
    /// Standard error of ops whose group has not been ported yet.
    pub fn not_implemented(op: &str) -> Self {
        OpError::Internal(format!("not implemented: {op}"))
    }
}

impl std::fmt::Display for OpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpError::Validation(m) => write!(f, "validation: {m}"),
            OpError::UnknownOp(m) => write!(f, "unknown op: {m}"),
            OpError::Internal(m) => write!(f, "internal: {m}"),
        }
    }
}

impl std::error::Error for OpError {}

/// Application-level failure: `{"ok": false, "error": message, ...}`.
///
/// `state` is attached by the few legacy ops that return the state next to an error
/// (e.g. `launch-options` -> "Unsupported launch option."); `extra` carries additional
/// top-level keys (e.g. `depotRequired`, `depotCommand` of `exe-swap/compatible`).
#[derive(Debug, Clone)]
pub struct AppError {
    pub message: String,
    pub state: Option<Box<State>>,
    pub extra: Map<String, Value>,
}

impl AppError {
    pub fn new(message: impl Into<String>) -> Self {
        AppError { message: message.into(), state: None, extra: Map::new() }
    }
    pub fn with_state(mut self, state: State) -> Self {
        self.state = Some(Box::new(state));
        self
    }
    pub fn with_extra(mut self, key: &str, value: Value) -> Self {
        self.extra.insert(key.to_string(), value);
        self
    }
    /// The legacy JSON body.
    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        m.insert("ok".into(), Value::Bool(false));
        m.insert("error".into(), Value::String(self.message.clone()));
        for (k, v) in &self.extra {
            m.insert(k.clone(), v.clone());
        }
        if let Some(s) = &self.state {
            m.insert("state".into(), serde_json::to_value(s.as_ref()).unwrap_or(Value::Null));
        }
        Value::Object(m)
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for AppError {}

impl From<std::io::Error> for AppError {
    /// Uses the Python-compatible text (`[Errno 13] Permission denied: ...` needs the path, so
    /// prefer [`crate::pyfmt::io_error_text`] when a path is known).
    fn from(e: std::io::Error) -> Self {
        AppError::new(crate::pyfmt::io_error_text(&e, None))
    }
}

/// Result type of typed op handlers.
pub type AppResult<T> = Result<T, AppError>;
