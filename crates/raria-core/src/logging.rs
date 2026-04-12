use anyhow::Result;
use chrono::Utc;
use serde_json::{Map, Value};
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

static STRUCTURED_LOG_FILE: OnceLock<Arc<Mutex<File>>> = OnceLock::new();
static STRUCTURED_LOG_CONTEXT: OnceLock<Arc<Mutex<Map<String, Value>>>> = OnceLock::new();

fn structured_log_context() -> &'static Arc<Mutex<Map<String, Value>>> {
    STRUCTURED_LOG_CONTEXT.get_or_init(|| Arc::new(Mutex::new(Map::new())))
}

/// Install the shared structured log sink used by the bounded rollout tranche.
pub fn install_structured_log_file(path: &Path) -> Result<()> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let _ = STRUCTURED_LOG_FILE.set(Arc::new(Mutex::new(file)));
    Ok(())
}

/// Replace the shared structured log context merged into all emitted log fields.
pub fn replace_structured_log_context(
    fields: impl IntoIterator<Item = (&'static str, String)>,
) -> Result<()> {
    let mut next = Map::new();
    for (key, value) in fields {
        next.insert(key.into(), Value::String(value));
    }

    let mut guard = structured_log_context()
        .lock()
        .map_err(|_| anyhow::anyhow!("structured log context mutex poisoned"))?;
    *guard = next;
    Ok(())
}

/// Build a structured lifecycle log payload as a JSON string.
pub fn lifecycle_event(
    level: &str,
    target: &str,
    message: &str,
    fields: impl IntoIterator<Item = (&'static str, String)>,
) -> String {
    let mut payload = Map::new();
    payload.insert("timestamp".into(), Value::String(Utc::now().to_rfc3339()));
    payload.insert("level".into(), Value::String(level.to_string()));
    payload.insert("target".into(), Value::String(target.to_string()));

    let mut field_map = Map::new();
    if let Ok(guard) = structured_log_context().lock() {
        field_map.extend(guard.clone());
    }
    for (key, value) in fields {
        field_map.insert(key.into(), Value::String(value));
    }
    payload.insert("fields".into(), Value::Object(field_map));
    payload.insert("message".into(), Value::String(message.to_string()));
    Value::Object(payload).to_string()
}

/// Emit a structured lifecycle event into the shared structured log sink, if installed.
pub fn emit_structured_log(
    level: &str,
    target: &str,
    message: &str,
    fields: impl IntoIterator<Item = (&'static str, String)>,
) {
    let Some(file) = STRUCTURED_LOG_FILE.get() else {
        return;
    };

    if let Ok(mut guard) = file.lock() {
        let _ = writeln!(guard, "{}", lifecycle_event(level, target, message, fields));
        let _ = guard.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::{emit_structured_log, lifecycle_event, replace_structured_log_context};

    #[test]
    fn lifecycle_event_emits_json_payload() {
        let payload = lifecycle_event(
            "INFO",
            "raria::engine",
            "job added",
            [("gid", "0000000000000001".to_string())],
        );
        let parsed: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(parsed["level"], "INFO");
        assert_eq!(parsed["target"], "raria::engine");
        assert_eq!(parsed["message"], "job added");
        assert_eq!(parsed["fields"]["gid"], "0000000000000001");
    }

    #[test]
    fn lifecycle_event_merges_global_context_fields() {
        replace_structured_log_context([("session_id", "session-123".to_string())]).unwrap();

        let payload = lifecycle_event("INFO", "raria::engine", "job added", []);
        let parsed: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(parsed["fields"]["session_id"], "session-123");

        replace_structured_log_context([]).unwrap();
    }

    #[test]
    fn emit_without_install_is_noop() {
        emit_structured_log(
            "INFO",
            "raria::engine",
            "noop",
            [("gid", "0000000000000001".to_string())],
        );
    }
}
