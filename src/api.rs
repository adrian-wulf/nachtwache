use crate::{
    ai::diagnose_issue,
    db::Database,
    models::AiSettings,
};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use serde_json::json;
use tracing::error;

#[derive(Deserialize)]
pub struct ListIssuesQuery {
    pub project_id: Option<String>,
    pub status: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Deserialize)]
pub struct UpdateStatusBody {
    pub status: String,
}

pub async fn list_issues(
    State(db): State<Database>,
    Query(query): Query<ListIssuesQuery>,
) -> impl IntoResponse {
    let limit = query.limit.unwrap_or(50);
    match db.list_issues(
        query.project_id.as_deref(),
        query.status.as_deref(),
        limit,
    ) {
        Ok(issues) => (StatusCode::OK, Json(json!({ "issues": issues }))),
        Err(e) => {
            error!("Failed to list issues: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        }
    }
}

pub async fn get_issue(
    State(db): State<Database>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match db.get_issue(&id) {
        Ok(Some((issue, events))) => (
            StatusCode::OK,
            Json(json!({
                "issue": issue,
                "events": events
            })),
        ),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Issue not found" })),
        ),
        Err(e) => {
            error!("Failed to get issue {}: {:?}", id, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        }
    }
}

pub async fn update_issue_status(
    State(db): State<Database>,
    Path(id): Path<String>,
    Json(body): Json<UpdateStatusBody>,
) -> impl IntoResponse {
    match db.update_issue_status(&id, &body.status) {
        Ok(_) => (StatusCode::OK, Json(json!({ "success": true }))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

pub async fn trigger_ai_diagnosis(
    State(db): State<Database>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let issue_and_events = match db.get_issue(&id) {
        Ok(Some(pair)) => pair,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "Issue not found" })),
            );
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            );
        }
    };

    let (issue, events) = issue_and_events;
    let latest_payload = events
        .first()
        .map(|e| e.payload.clone())
        .unwrap_or(json!({}));

    let ai_settings = match db.get_ai_settings() {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("Failed to read AI settings: {}", e) })),
            );
        }
    };

    match diagnose_issue(&issue, &latest_payload, &ai_settings).await {
        Ok(diag) => {
            let diagnosis_text = format!("{}\n\n**Root cause:** {}", diag.explanation, diag.root_cause);
            let _ = db.save_ai_analysis(&id, &diagnosis_text, diag.diff.as_deref());

            (StatusCode::OK, Json(json!({ "diagnosis": diag })))
        }
        Err(e) => {
            error!("AI diagnosis failed for issue {}: {}", id, e);
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": format!("AI diagnosis failed: {}", e) })),
            )
        }
    }
}

pub async fn get_stats(State(db): State<Database>) -> impl IntoResponse {
    match db.get_stats() {
        Ok(stats) => (StatusCode::OK, Json(json!(stats))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

pub async fn get_ai_settings(State(db): State<Database>) -> impl IntoResponse {
    match db.get_ai_settings() {
        Ok(s) => {
            // Mask API key if set for display, but keep hint
            let has_key = !s.api_key.is_empty();
            let masked_key = if has_key {
                format!("••••••••{}", &s.api_key[s.api_key.len().saturating_sub(4)..])
            } else {
                "".to_string()
            };
            let mut response_val = json!(s);
            response_val["has_key"] = json!(has_key);
            response_val["masked_key"] = json!(masked_key);
            (StatusCode::OK, Json(response_val))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

pub async fn save_ai_settings(
    State(db): State<Database>,
    Json(mut new_settings): Json<AiSettings>,
) -> impl IntoResponse {
    // If incoming api_key is masked or empty, preserve existing key
    if new_settings.api_key.starts_with("••••") || new_settings.api_key.is_empty() {
        if let Ok(existing) = db.get_ai_settings() {
            new_settings.api_key = existing.api_key;
        }
    }

    match db.save_ai_settings(&new_settings) {
        Ok(_) => (StatusCode::OK, Json(json!({ "success": true }))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

pub async fn get_project_info(State(db): State<Database>) -> impl IntoResponse {
    match db.get_default_project() {
        Ok(project) => (StatusCode::OK, Json(json!(project))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

pub async fn clear_all_events(State(db): State<Database>) -> impl IntoResponse {
    match db.clear_all() {
        Ok(_) => (StatusCode::OK, Json(json!({ "success": true }))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}
