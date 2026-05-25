//! Shared-хелперы для парсинга SC payload v1 JSON. Используются всеми
//! `ScXxxFields::from_sc` (треки/плейлисты/юзеры) и SC-shape проекциями.

use chrono::{DateTime, Utc};
use serde_json::{json, Value};

/// Достать строковое поле, обрезать пробелы, отфильтровать пустое.
pub fn string_field(payload: &Value, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(String::from)
}

/// Распарсить RFC3339 datetime (опционально с SC-формой `YYYY/MM/DD HH:MM:SS +0000`).
pub fn parse_dt(value: Option<&Value>) -> Option<DateTime<Utc>> {
    let s = value.and_then(|v| v.as_str())?;
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(s, "%Y/%m/%d %H:%M:%S %z")
                .ok()
                .map(|naive| DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc))
        })
}

/// SC ID в shape-проекциях: числовая часть — как `Number`, чтобы не ломать
/// клиентские контракты; всё остальное — `String`.
pub fn parse_id_or_string(s: &str) -> Value {
    s.parse::<i64>()
        .map(|n| json!(n))
        .unwrap_or_else(|_| Value::String(s.to_string()))
}
