use crate::db::Database;
use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde_json::{json, Value};
use tracing::{error, info, warn};

pub async fn handle_envelope(
    State(db): State<Database>,
    Path(project_id): Path<String>,
    _headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let body_str = match String::from_utf8(body.to_vec()) {
        Ok(s) => s,
        Err(e) => {
            warn!("Failed to read envelope body as UTF-8: {:?}", e);
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "invalid utf-8 body"})),
            );
        }
    };

    let mut lines = body_str.lines();

    // 1. Envelope Header
    let env_header_line = match lines.next() {
        Some(line) => line,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "empty envelope"})),
            );
        }
    };

    let env_header: Value = match serde_json::from_str(env_header_line) {
        Ok(v) => v,
        Err(e) => {
            warn!("Invalid envelope header JSON: {:?}", e);
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "invalid envelope header"})),
            );
        }
    };

    let envelope_event_id = env_header
        .get("event_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let mut processed_event_id = envelope_event_id.clone();

    // 2. Parse Items
    while let Some(item_header_line) = lines.next() {
        if item_header_line.trim().is_empty() {
            continue;
        }

        let item_header: Value = match serde_json::from_str(item_header_line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let item_type = item_header
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("event");

        let item_payload_line = match lines.next() {
            Some(line) => line,
            None => break,
        };

        match item_type {
            "event" => {
                if let Ok(event_payload) = serde_json::from_str::<Value>(item_payload_line) {
                    let ev_id = event_payload
                        .get("event_id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| envelope_event_id.clone());

                    processed_event_id = ev_id.clone();

                    if let Err(e) = process_and_store_event(&db, &project_id, &ev_id, &event_payload) {
                        error!("Failed to store event: {:?}", e);
                    }
                }
            }
            "transaction" => {
                // APM / Performance tracing
                info!("Ingested transaction performance trace for project {}", project_id);
            }
            "session" | "client_report" => {
                // Heartbeat or telemetry
            }
            other => {
                info!("Unhandled envelope item type: {}", other);
            }
        }
    }

    // Sentry SDK expects 200 OK with {"id": "<event_id>"}
    (
        StatusCode::OK,
        Json(json!({ "id": processed_event_id })),
    )
}

pub async fn handle_store(
    State(db): State<Database>,
    Path(project_id): Path<String>,
    _headers: HeaderMap,
    Json(event_payload): Json<Value>,
) -> impl IntoResponse {
    let event_id = event_payload
        .get("event_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    if let Err(e) = process_and_store_event(&db, &project_id, &event_id, &event_payload) {
        error!("Failed to store legacy event: {:?}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "failed to record event"})),
        );
    }

    (
        StatusCode::OK,
        Json(json!({ "id": event_id })),
    )
}

fn process_and_store_event(
    db: &Database,
    project_id: &str,
    event_id: &str,
    payload: &Value,
) -> Result<(), rusqlite::Error> {
    let level = payload
        .get("level")
        .and_then(|v| v.as_str())
        .unwrap_or("error");

    let platform = payload
        .get("platform")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    // Extract title & culprit from exception or message
    let (title, culprit, fingerprint_hint) = extract_title_and_culprit(payload);

    let fingerprint = if let Some(arr) = payload.get("fingerprint").and_then(|v| v.as_array()) {
        arr.iter()
            .filter_map(|item| item.as_str())
            .collect::<Vec<_>>()
            .join(":")
    } else {
        fingerprint_hint
    };

    let (issue_id, _) = db.record_event(
        project_id,
        event_id,
        &fingerprint,
        &title,
        &culprit,
        level,
        platform,
        payload,
    )?;

    info!(
        "Recorded event {} for issue {} [{}]: {}",
        event_id, issue_id, level, title
    );

    Ok(())
}

fn extract_title_and_culprit(payload: &Value) -> (String, String, String) {
    let mut title = String::new();
    let mut culprit = String::new();

    // 1. Try exceptions
    if let Some(values) = payload
        .get("exception")
        .and_then(|e| e.get("values"))
        .and_then(|v| v.as_array())
    {
        if let Some(exc) = values.last() {
            let exc_type = exc
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("Error");
            let exc_val = exc
                .get("value")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if !exc_val.is_empty() {
                title = format!("{}: {}", exc_type, exc_val);
            } else {
                title = exc_type.to_string();
            }

            // Extract culprit from stacktrace
            if let Some(frames) = exc
                .get("stacktrace")
                .and_then(|s| s.get("frames"))
                .and_then(|f| f.as_array())
            {
                // Prefer last in-app frame, else last frame
                let top_frame = frames
                    .iter()
                    .rev()
                    .find(|f| f.get("in_app").and_then(|b| b.as_bool()).unwrap_or(false))
                    .or_else(|| frames.last());

                if let Some(frame) = top_frame {
                    let filename = frame
                        .get("filename")
                        .or_else(|| frame.get("module"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let lineno = frame
                        .get("lineno")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    let func = frame
                        .get("function")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");

                    if !func.is_empty() {
                        culprit = format!("{}:{} in {}", filename, lineno, func);
                    } else {
                        culprit = format!("{}:{}", filename, lineno);
                    }
                }
            }
        }
    }

    // 2. Fallback to message
    if title.is_empty() {
        if let Some(msg) = payload.get("message").and_then(|m| {
            m.as_str()
                .map(|s| s.to_string())
                .or_else(|| m.get("formatted").and_then(|f| f.as_str()).map(|s| s.to_string()))
        }) {
            title = msg;
        } else {
            title = "Unknown Event".to_string();
        }
    }

    // 3. Fallback to transaction / culprit in payload
    if culprit.is_empty() {
        if let Some(c) = payload.get("culprit").and_then(|v| v.as_str()) {
            culprit = c.to_string();
        } else if let Some(t) = payload.get("transaction").and_then(|v| v.as_str()) {
            culprit = t.to_string();
        } else {
            culprit = "unknown".to_string();
        }
    }

    let fingerprint_hint = format!("{}:{}", title, culprit);
    (title, culprit, fingerprint_hint)
}
