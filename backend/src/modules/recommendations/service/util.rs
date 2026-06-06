use qdrant_client::qdrant::{point_id::PointIdOptions, PointId, Value as QValue};
use serde_json::{json, Value};
use std::collections::HashMap;

pub(crate) fn parse_id_or_null(raw: &str) -> Option<u64> {
    let s = raw.trim();
    let last = match s.rsplit_once(':') {
        Some((_, t)) => t,
        None => s,
    };
    if !last.bytes().all(|b| b.is_ascii_digit()) || last.is_empty() {
        return None;
    }
    last.parse::<u64>().ok()
}

/// SoundCloud user-id живёт в двух формах — URN (`soundcloud:users:123`) и
/// голой (`123`). На проде лайки/события расщеплены по обеим; чтобы волна
/// видела ВСЕ сигналы юзера, матчим по обоим вариантам (`user_id = ANY(...)`).
pub(crate) fn user_id_variants(sc_user_id: &str) -> Vec<String> {
    let trimmed = sc_user_id.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let bare = trimmed.rsplit(':').next().unwrap_or(trimmed);
    let mut out = vec![trimmed.to_string()];
    if bare != trimmed {
        out.push(bare.to_string());
    }
    if bare.bytes().all(|b| b.is_ascii_digit()) && !bare.is_empty() {
        let urn = format!("soundcloud:users:{bare}");
        if urn != trimmed {
            out.push(urn);
        }
    }
    out.dedup();
    out
}

pub(crate) fn numeric_id(id: u64) -> PointId {
    PointId {
        point_id_options: Some(PointIdOptions::Num(id)),
    }
}

pub(crate) fn point_id_to_value(id: Option<PointId>) -> Value {
    match id.and_then(|id| id.point_id_options) {
        Some(PointIdOptions::Num(n)) => json!(n),
        Some(PointIdOptions::Uuid(u)) => json!(u),
        None => Value::Null,
    }
}

pub(crate) fn value_to_u64(v: &Value) -> Option<u64> {
    if let Some(n) = v.as_u64() {
        return Some(n);
    }
    if let Some(s) = v.as_str() {
        return s.parse::<u64>().ok();
    }
    None
}

pub(crate) fn value_id_to_string(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        return s.to_string();
    }
    if let Some(n) = v.as_u64() {
        return n.to_string();
    }
    v.to_string()
}

pub(crate) fn payload_to_map(p: HashMap<String, QValue>) -> HashMap<String, Value> {
    let mut out = HashMap::new();
    for (k, v) in p {
        out.insert(k, qvalue_to_value(v));
    }
    out
}

fn qvalue_to_value(v: QValue) -> Value {
    use qdrant_client::qdrant::value::Kind;
    match v.kind {
        Some(Kind::NullValue(_)) => Value::Null,
        Some(Kind::BoolValue(b)) => Value::Bool(b),
        Some(Kind::IntegerValue(i)) => json!(i),
        Some(Kind::DoubleValue(d)) => json!(d),
        Some(Kind::StringValue(s)) => Value::String(s),
        Some(Kind::ListValue(l)) => {
            Value::Array(l.values.into_iter().map(qvalue_to_value).collect())
        }
        Some(Kind::StructValue(s)) => {
            let mut m = serde_json::Map::new();
            for (k, val) in s.fields {
                m.insert(k, qvalue_to_value(val));
            }
            Value::Object(m)
        }
        None => Value::Null,
    }
}
