use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tauri::AppHandle;
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::{EnvFilter, fmt as subscriber_fmt, util::SubscriberInitExt};
use zip::write::SimpleFileOptions;

use crate::config::{self, DiagnosticsLogLevel, DiagnosticsSettings};
use crate::core::sftp;
use crate::core::{SessionInfo, SessionManager, SessionType};
use crate::error::{AppError, AppResult};

pub const LOG_FILE_PREFIX: &str = "nyaterm-diagnostics";
pub const LOG_FILE_SUFFIX: &str = "jsonl";

/// Identifies which binary produced a log file.
///
/// Hand-built test packages and the official release share a version number and
/// a product name, so the startup record carries an explicit tag: it is the only
/// reliable way to tell from a user's log which build was actually running.
pub const BUILD_TAG: &str = "tmux-cc-test-2026-09-30";

const DEFAULT_RETENTION_DAYS: u32 = 7;
const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(30);

lazy_static::lazy_static! {
    static ref RATE_LIMITS: Mutex<HashMap<String, RateLimitState>> = Mutex::new(HashMap::new());
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StructuredLogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FrontendLogEntry {
    #[serde(default)]
    pub timestamp: Option<String>,
    pub level: StructuredLogLevel,
    pub domain: String,
    pub event: String,
    pub message: String,
    #[serde(default)]
    pub ids: Option<Value>,
    #[serde(default)]
    pub data: Option<Value>,
    #[serde(default)]
    pub error: Option<Value>,
}

#[derive(Clone, Debug)]
pub struct StructuredLog {
    pub level: StructuredLogLevel,
    pub domain: String,
    pub event: String,
    pub message: String,
    pub ids: Option<Value>,
    pub data: Option<Value>,
    pub error: Option<Value>,
    pub client_timestamp: Option<String>,
}

#[derive(Clone, Debug)]
struct RateLimitState {
    last_emitted_at: Instant,
    suppressed_count: u32,
}

#[derive(Clone, Default)]
struct StructuredJsonFormatter;

#[derive(Default)]
struct JsonFieldVisitor {
    fields: Map<String, Value>,
}

pub fn init_tracing(log_dir: PathBuf, settings: &DiagnosticsSettings) {
    let retention_days = normalize_retention_days(settings.retention_days);
    let _ = std::fs::create_dir_all(&log_dir);
    let _ = cleanup_old_logs(&log_dir, retention_days);

    let file_appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix(LOG_FILE_PREFIX)
        .filename_suffix(LOG_FILE_SUFFIX)
        .max_log_files(retention_days as usize)
        .build(&log_dir)
        .expect("failed to initialize rolling file appender");

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(default_filter(settings.level)));

    let local_time = subscriber_fmt::time::OffsetTime::local_rfc_3339().unwrap_or_else(|_| {
        subscriber_fmt::time::OffsetTime::new(
            time::UtcOffset::UTC,
            time::format_description::well_known::Rfc3339,
        )
    });

    let file_layer = subscriber_fmt::layer()
        .event_format(StructuredJsonFormatter)
        .with_writer(file_appender)
        .with_ansi(false);

    let subscriber = tracing_subscriber::registry().with(filter).with(file_layer);

    if cfg!(debug_assertions) {
        subscriber
            .with(
                subscriber_fmt::layer()
                    .with_writer(std::io::stderr)
                    .compact()
                    .with_timer(local_time),
            )
            .init();
    } else {
        subscriber.init();
    }

    log_event(StructuredLog {
        level: StructuredLogLevel::Info,
        domain: "app.lifecycle".to_string(),
        event: "app.start".to_string(),
        message: "NyaTerm starting".to_string(),
        ids: None,
        data: Some(json!({
            "log_level": settings.level.as_str(),
            "retention_days": retention_days,
            "build": BUILD_TAG,
        })),
        error: None,
        client_timestamp: None,
    });
}

pub fn log_event(log: StructuredLog) {
    let ids = serialize_json_value(log.ids);
    let data = serialize_json_value(log.data);
    let error = serialize_json_value(log.error);
    let client_timestamp = log.client_timestamp.unwrap_or_default();
    match log.level {
        StructuredLogLevel::Debug => tracing::event!(
            Level::DEBUG,
            domain = log.domain.as_str(),
            event = log.event.as_str(),
            message = log.message.as_str(),
            ids = ids.as_deref().unwrap_or(""),
            data = data.as_deref().unwrap_or(""),
            error = error.as_deref().unwrap_or(""),
            client_timestamp = client_timestamp.as_str(),
        ),
        StructuredLogLevel::Info => tracing::event!(
            Level::INFO,
            domain = log.domain.as_str(),
            event = log.event.as_str(),
            message = log.message.as_str(),
            ids = ids.as_deref().unwrap_or(""),
            data = data.as_deref().unwrap_or(""),
            error = error.as_deref().unwrap_or(""),
            client_timestamp = client_timestamp.as_str(),
        ),
        StructuredLogLevel::Warn => tracing::event!(
            Level::WARN,
            domain = log.domain.as_str(),
            event = log.event.as_str(),
            message = log.message.as_str(),
            ids = ids.as_deref().unwrap_or(""),
            data = data.as_deref().unwrap_or(""),
            error = error.as_deref().unwrap_or(""),
            client_timestamp = client_timestamp.as_str(),
        ),
        StructuredLogLevel::Error => tracing::event!(
            Level::ERROR,
            domain = log.domain.as_str(),
            event = log.event.as_str(),
            message = log.message.as_str(),
            ids = ids.as_deref().unwrap_or(""),
            data = data.as_deref().unwrap_or(""),
            error = error.as_deref().unwrap_or(""),
            client_timestamp = client_timestamp.as_str(),
        ),
    }
}

pub fn log_rate_limited(log: StructuredLog) {
    let key = rate_limit_key(&log);
    let now = Instant::now();
    let mut should_log = true;
    let mut summary_count = None;

    {
        let mut states = RATE_LIMITS.lock().unwrap();
        match states.get_mut(&key) {
            Some(state) if now.duration_since(state.last_emitted_at) < RATE_LIMIT_WINDOW => {
                state.suppressed_count += 1;
                should_log = false;
            }
            Some(state) => {
                if state.suppressed_count > 0 {
                    summary_count = Some(state.suppressed_count);
                }
                state.last_emitted_at = now;
                state.suppressed_count = 0;
            }
            None => {
                states.insert(
                    key,
                    RateLimitState {
                        last_emitted_at: now,
                        suppressed_count: 0,
                    },
                );
            }
        }
    }

    if let Some(suppressed_count) = summary_count {
        let mut data = log
            .data
            .clone()
            .unwrap_or_else(|| Value::Object(Map::new()));
        if let Value::Object(ref mut map) = data {
            map.insert(
                "suppressed_count".to_string(),
                Value::Number(serde_json::Number::from(suppressed_count)),
            );
            map.insert(
                "suppressed_window_seconds".to_string(),
                Value::Number(serde_json::Number::from(RATE_LIMIT_WINDOW.as_secs())),
            );
        }
        log_event(StructuredLog {
            level: log.level,
            domain: log.domain.clone(),
            event: format!("{}.suppressed", log.event),
            message: "Suppressed repeated log entries".to_string(),
            ids: log.ids.clone(),
            data: Some(data),
            error: None,
            client_timestamp: None,
        });
    }

    if should_log {
        log_event(log);
    }
}

pub fn export_diagnostics(
    app: &AppHandle,
    session_manager: &SessionManager,
    tunnel_manager: &crate::core::ssh::TunnelManager,
    output_path: &str,
) -> AppResult<()> {
    let settings = config::load_app_settings(app).unwrap_or_default();
    let log_dir = crate::runtime::log_dir(app)?;
    let log_files = collect_log_files(
        &log_dir,
        normalize_retention_days(settings.diagnostics.retention_days),
    )?;

    if let Some(parent) = Path::new(output_path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let file = std::fs::File::create(output_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    for log_file in log_files {
        let contents = std::fs::read(&log_file)?;
        let file_name = log_file
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| AppError::Config("invalid log file name".to_string()))?;
        zip.start_file(file_name, options)
            .map_err(|e| AppError::Config(format!("zip write: {e}")))?;
        use std::io::Write;
        zip.write_all(&contents)?;
    }

    let manifest = build_manifest(app, &settings);
    zip.start_file("manifest.json", options)
        .map_err(|e| AppError::Config(format!("zip write: {e}")))?;
    use std::io::Write;
    zip.write_all(
        serde_json::to_string_pretty(&manifest)
            .map_err(|e| AppError::Config(format!("serialize manifest: {e}")))?
            .as_bytes(),
    )?;

    let runtime_snapshot = build_runtime_snapshot(app, &settings, session_manager, tunnel_manager);
    zip.start_file("runtime_snapshot.json", options)
        .map_err(|e| AppError::Config(format!("zip write: {e}")))?;
    zip.write_all(
        serde_json::to_string_pretty(&runtime_snapshot)
            .map_err(|e| AppError::Config(format!("serialize runtime snapshot: {e}")))?
            .as_bytes(),
    )?;

    zip.finish()
        .map_err(|e| AppError::Config(format!("zip finalize: {e}")))?;

    Ok(())
}

pub fn frontend_log_to_structured(entry: FrontendLogEntry) -> StructuredLog {
    StructuredLog {
        level: entry.level,
        domain: entry.domain,
        event: entry.event,
        message: entry.message,
        ids: entry.ids,
        data: entry.data,
        error: entry.error,
        client_timestamp: entry.timestamp,
    }
}

impl<S, N> FormatEvent<S, N> for StructuredJsonFormatter
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    N: for<'writer> FormatFields<'writer> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let metadata = event.metadata();
        let mut visitor = JsonFieldVisitor::default();
        event.record(&mut visitor);

        let entry = build_log_entry_json(
            &metadata.level().to_string().to_ascii_lowercase(),
            metadata.target(),
            visitor.fields,
            format_timestamp(),
        );
        let serialized = serde_json::to_string(&entry).map_err(|_| fmt::Error)?;
        writer.write_str(&serialized)?;
        writer.write_str("\n")
    }
}

impl tracing::field::Visit for JsonFieldVisitor {
    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.fields.insert(
            field.name().to_string(),
            Value::Number(serde_json::Number::from(value)),
        );
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.fields.insert(
            field.name().to_string(),
            Value::Number(serde_json::Number::from(value)),
        );
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.fields
            .insert(field.name().to_string(), Value::Bool(value));
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.fields
            .insert(field.name().to_string(), Value::String(value.to_string()));
    }

    fn record_error(
        &mut self,
        field: &tracing::field::Field,
        value: &(dyn std::error::Error + 'static),
    ) {
        self.fields
            .insert(field.name().to_string(), Value::String(value.to_string()));
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        self.fields.insert(
            field.name().to_string(),
            Value::String(format!("{value:?}")),
        );
    }

    fn record_f64(&mut self, field: &tracing::field::Field, value: f64) {
        if let Some(number) = serde_json::Number::from_f64(value) {
            self.fields
                .insert(field.name().to_string(), Value::Number(number));
        }
    }
}

fn build_log_entry_json(
    level: &str,
    target: &str,
    mut fields: Map<String, Value>,
    timestamp: String,
) -> Value {
    let domain = take_string_field(&mut fields, "domain").unwrap_or_else(|| target.to_string());
    let event =
        take_string_field(&mut fields, "event").unwrap_or_else(|| "unclassified".to_string());
    let message = take_string_field(&mut fields, "message").unwrap_or_else(|| target.to_string());
    let client_timestamp = take_string_field(&mut fields, "client_timestamp");

    let ids = parse_reserved_json_field(fields.remove("ids"));
    let error = parse_reserved_json_field(fields.remove("error"));
    let mut data = match parse_reserved_json_field(fields.remove("data")) {
        Some(Value::Object(map)) => map,
        Some(other) => {
            let mut map = Map::new();
            map.insert("value".to_string(), other);
            map
        }
        None => Map::new(),
    };

    if let Some(client_timestamp) = client_timestamp {
        data.insert(
            "client_timestamp".to_string(),
            Value::String(client_timestamp),
        );
    }

    for (key, value) in fields {
        if matches!(value, Value::String(ref text) if text.is_empty()) {
            continue;
        }
        data.insert(key, value);
    }

    let mut entry = Map::new();
    entry.insert("timestamp".to_string(), Value::String(timestamp));
    entry.insert("level".to_string(), Value::String(level.to_string()));
    entry.insert("domain".to_string(), Value::String(domain));
    entry.insert("event".to_string(), Value::String(event));
    entry.insert(
        "message".to_string(),
        sanitize_value(Some("message"), Value::String(message)),
    );
    if let Some(ids) = ids {
        entry.insert("ids".to_string(), sanitize_value(Some("ids"), ids));
    }
    if !data.is_empty() {
        entry.insert(
            "data".to_string(),
            sanitize_value(Some("data"), Value::Object(data)),
        );
    }
    if let Some(error) = error {
        entry.insert("error".to_string(), sanitize_value(Some("error"), error));
    }

    Value::Object(entry)
}

fn build_manifest(app: &AppHandle, settings: &config::AppSettings) -> Value {
    json!({
        "app_version": app.package_info().version.to_string(),
        "platform": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "language": settings.ui.language.clone().unwrap_or_else(|| "en".to_string()),
        "log_level": settings.diagnostics.level.as_str(),
        "retention_days": normalize_retention_days(settings.diagnostics.retention_days),
        "exported_at": format_timestamp(),
    })
}

fn build_runtime_snapshot(
    app: &AppHandle,
    settings: &config::AppSettings,
    session_manager: &SessionManager,
    tunnel_manager: &crate::core::ssh::TunnelManager,
) -> Value {
    let sessions = tauri::async_runtime::block_on(session_manager.list_sessions());
    let tunnel_count = tauri::async_runtime::block_on(tunnel_manager.active_count());
    let saved_tunnels = config::load_tunnels(app)
        .map(|items| items.len())
        .unwrap_or(0);
    let session_summary = summarize_sessions(&sessions);

    json!({
        "diagnostics": {
            "level": settings.diagnostics.level.as_str(),
            "retention_days": normalize_retention_days(settings.diagnostics.retention_days),
        },
        "ui": {
            "language": settings.ui.language.clone().unwrap_or_else(|| "en".to_string()),
            "active_left_panel": settings.ui.active_left_panel,
            "active_right_panel": settings.ui.active_right_panel,
            "show_quick_cmd_bar": settings.ui.show_quick_cmd_bar,
            "show_serial_send_panel": settings.ui.show_serial_send_panel,
            "show_remote_stats": settings.ui.show_remote_stats,
            "show_gpu_monitor": settings.ui.show_gpu_monitor,
            "show_ascend_npu_monitor": settings.ui.show_ascend_npu_monitor,
            "open_tab_count": settings.ui.open_tabs.len(),
        },
        "session_summary": session_summary,
        "transfer_summary": {
            "active_count": sftp::active_transfer_count(),
        },
        "tunnel_summary": {
            "active_count": tunnel_count,
            "saved_count": saved_tunnels,
        },
    })
}

fn summarize_sessions(sessions: &[SessionInfo]) -> Value {
    let mut summary = json!({
        "active_total": sessions.len(),
        "ssh": 0,
        "local": 0,
        "telnet": 0,
        "serial": 0,
        "cwd_tracking_enabled": 0,
    });

    if let Value::Object(ref mut map) = summary {
        for session in sessions {
            let key = match session.session_type {
                SessionType::SSH => "ssh",
                SessionType::Local => "local",
                SessionType::Telnet => "telnet",
                SessionType::Serial => "serial",
            };
            increment_counter(map, key);
            if session.injection_active {
                increment_counter(map, "cwd_tracking_enabled");
            }
        }
    }

    summary
}

fn increment_counter(map: &mut Map<String, Value>, key: &str) {
    let next = map
        .get(key)
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .saturating_add(1);
    map.insert(
        key.to_string(),
        Value::Number(serde_json::Number::from(next)),
    );
}

fn collect_log_files(log_dir: &Path, retention_days: u32) -> AppResult<Vec<PathBuf>> {
    let min_modified = threshold_system_time(retention_days);
    let mut files = Vec::new();
    for entry in std::fs::read_dir(log_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !is_log_file(&path) {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        if modified < min_modified {
            continue;
        }
        files.push(path);
    }
    files.sort();
    Ok(files)
}

fn cleanup_old_logs(log_dir: &Path, retention_days: u32) -> AppResult<()> {
    let min_modified = threshold_system_time(retention_days);
    for entry in std::fs::read_dir(log_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !is_log_file(&path) {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        if modified < min_modified {
            let _ = std::fs::remove_file(path);
        }
    }
    Ok(())
}

fn threshold_system_time(retention_days: u32) -> SystemTime {
    SystemTime::now()
        .checked_sub(Duration::from_secs(
            u64::from(retention_days) * 24 * 60 * 60,
        ))
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

fn is_log_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.starts_with(LOG_FILE_PREFIX) && value.ends_with(LOG_FILE_SUFFIX))
}

fn serialize_json_value(value: Option<Value>) -> Option<String> {
    value.and_then(|item| serde_json::to_string(&item).ok())
}

fn parse_reserved_json_field(value: Option<Value>) -> Option<Value> {
    match value {
        Some(Value::String(text)) if text.is_empty() => None,
        Some(Value::String(text)) => serde_json::from_str(&text)
            .ok()
            .or_else(|| Some(Value::String(text))),
        Some(other) => Some(other),
        None => None,
    }
}

fn take_string_field(fields: &mut Map<String, Value>, key: &str) -> Option<String> {
    match fields.remove(key) {
        Some(Value::String(value)) if !value.is_empty() => Some(value),
        Some(other) => Some(other.to_string()),
        None => None,
    }
}

fn rate_limit_key(log: &StructuredLog) -> String {
    let ids = log
        .ids
        .as_ref()
        .and_then(|value| serde_json::to_string(value).ok())
        .unwrap_or_default();
    let error = log
        .error
        .as_ref()
        .and_then(normalized_error_key)
        .unwrap_or_default();
    format!("{}|{}|{}", log.event, ids, error)
}

fn normalized_error_key(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        other => serde_json::to_string(other).ok(),
    }
}

fn sanitize_value(key: Option<&str>, value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(child_key, child_value)| {
                    let sanitized = sanitize_value(Some(&child_key), child_value);
                    (child_key, sanitized)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|item| sanitize_value(key, item))
                .collect(),
        ),
        Value::String(text) => sanitize_string(key, text),
        other => other,
    }
}

fn sanitize_string(key: Option<&str>, value: String) -> Value {
    let Some(key) = key else {
        return Value::String(value);
    };

    if is_redacted_key(key) {
        return Value::String("[REDACTED]".to_string());
    }

    if is_host_key(key) {
        return json!({
            "type": "host",
            "hash": stable_hash("host", &value),
        });
    }

    if is_username_key(key) {
        return json!({
            "type": "username",
            "hash": stable_hash("username", &value),
        });
    }

    if is_path_key(key) {
        let extension = Path::new(&value)
            .extension()
            .and_then(|item| item.to_str())
            .map(ToString::to_string);
        let mut map = Map::new();
        map.insert("type".to_string(), Value::String("path".to_string()));
        map.insert(
            "hash".to_string(),
            Value::String(stable_hash("path", &value)),
        );
        if let Some(extension) = extension {
            map.insert("extension".to_string(), Value::String(extension));
        }
        return Value::Object(map);
    }

    Value::String(value)
}

fn is_redacted_key(key: &str) -> bool {
    let key = normalize_key(key);
    matches!(
        key.as_str(),
        "password"
            | "secret"
            | "token"
            | "otp"
            | "command"
            | "content"
            | "clipboard"
            | "passphrase"
            | "master_password"
    ) || key.ends_with("_password")
        || key.ends_with("_secret")
        || key.ends_with("_token")
        || key.ends_with("_otp")
        || key.ends_with("_command")
        || key.ends_with("_content")
        || key.ends_with("_clipboard")
        || key.ends_with("_passphrase")
        || key.contains("private_key")
        || key.contains("public_key")
        || key.contains("key_data")
        || key.contains("secret_key")
        || (key.ends_with("_key") && !key.ends_with("_id"))
}

fn is_host_key(key: &str) -> bool {
    let key = normalize_key(key);
    key == "host" || key.ends_with("_host")
}

fn is_username_key(key: &str) -> bool {
    let key = normalize_key(key);
    key == "username" || key == "user" || key.ends_with("_username")
}

fn is_path_key(key: &str) -> bool {
    let key = normalize_key(key);
    key == "path" || key == "cwd" || key.ends_with("_path") || key.ends_with("_dir")
}

fn normalize_key(key: &str) -> String {
    key.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

fn stable_hash(kind: &str, value: &str) -> String {
    use sha2::Digest as _;

    let mut hasher = sha2::Sha256::new();
    hasher.update(kind.as_bytes());
    hasher.update(b":");
    hasher.update(value.as_bytes());
    let digest = hasher.finalize();
    hex::encode(&digest[..8])
}

fn default_filter(level: DiagnosticsLogLevel) -> String {
    let app_level = match level {
        DiagnosticsLogLevel::Warn => "warn",
        DiagnosticsLogLevel::Info => "info",
        DiagnosticsLogLevel::Debug => "debug",
    };
    format!("nyaterm={app_level},nyaterm_lib={app_level},warn")
}

fn normalize_retention_days(retention_days: u32) -> u32 {
    match retention_days {
        0 => DEFAULT_RETENTION_DAYS,
        value => value.min(30),
    }
}

fn format_timestamp() -> String {
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    now.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| now.unix_timestamp().to_string())
}

#[cfg(test)]
mod tests {
    use super::{Value, build_log_entry_json, sanitize_value};
    use serde_json::{Map, json};

    #[test]
    fn sanitize_value_redacts_secret_fields() {
        let value = json!({
            "password": "hunter2",
            "remote_path": "/var/log/app.log",
            "host": "example.com",
            "username": "root",
        });

        let sanitized = sanitize_value(Some("data"), value);
        assert_eq!(sanitized["password"], "[REDACTED]");
        assert_eq!(sanitized["remote_path"]["type"], "path");
        assert_eq!(sanitized["host"]["type"], "host");
        assert_eq!(sanitized["username"]["type"], "username");
    }

    #[test]
    fn build_log_entry_merges_reserved_and_extra_fields() {
        let mut fields = Map::new();
        fields.insert("domain".to_string(), Value::String("ui.error".to_string()));
        fields.insert(
            "event".to_string(),
            Value::String("dialog.open_failed".to_string()),
        );
        fields.insert(
            "message".to_string(),
            Value::String("Open failed".to_string()),
        );
        fields.insert(
            "ids".to_string(),
            Value::String("{\"session_id\":\"abc\"}".to_string()),
        );
        fields.insert(
            "data".to_string(),
            Value::String("{\"action\":\"open\"}".to_string()),
        );
        fields.insert("attempt".to_string(), Value::Number(1u64.into()));

        let entry = build_log_entry_json(
            "error",
            "nyaterm_lib::ui",
            fields,
            "2026-01-01T00:00:00Z".to_string(),
        );

        assert_eq!(entry["domain"], "ui.error");
        assert_eq!(entry["event"], "dialog.open_failed");
        assert_eq!(entry["ids"]["session_id"], "abc");
        assert_eq!(entry["data"]["action"], "open");
        assert_eq!(entry["data"]["attempt"], 1);
    }
}
