use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub public_key: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    pub id: String,
    pub project_id: String,
    pub fingerprint: String,
    pub title: String,
    pub culprit: String,
    pub level: String,
    pub platform: String,
    pub status: String, // unresolved, resolved, ignored
    pub count: i64,
    pub first_seen: String,
    pub last_seen: String,
    pub ai_diagnosis: Option<String>,
    pub ai_fix_diff: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventRecord {
    pub id: String,
    pub issue_id: String,
    pub project_id: String,
    pub payload: serde_json::Value,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsSummary {
    pub total_issues: i64,
    pub unresolved_issues: i64,
    pub total_events: i64,
    pub events_24h: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiSettings {
    pub provider: String, // "gemini", "openai", "anthropic", "ollama"
    pub api_key: String,
    pub model: String,
    pub endpoint: String,
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            provider: "ollama".to_string(),
            api_key: "".to_string(),
            model: "deepseek-4.1-flash".to_string(),
            endpoint: "http://localhost:11434".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiDiagnosisResponse {
    pub explanation: String,
    pub root_cause: String,
    pub diff: Option<String>,
    pub suggested_test: Option<String>,
}
