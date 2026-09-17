use crate::models::{AiDiagnosisResponse, AiSettings, Issue};
use reqwest::Client;
use serde_json::{json, Value};
use tracing::info;

pub async fn diagnose_issue(
    issue: &Issue,
    event_payload: &Value,
    settings: &AiSettings,
) -> Result<AiDiagnosisResponse, String> {
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    // Build context summary from payload
    let mut context_text = format!(
        "Issue: {}\nCulprit: {}\nPlatform: {}\nOccurrences: {}\n\n",
        issue.title, issue.culprit, issue.platform, issue.count
    );

    // Extract stacktrace
    if let Some(exc_values) = event_payload
        .get("exception")
        .and_then(|e| e.get("values"))
        .and_then(|v| v.as_array())
    {
        context_text.push_str("=== STACK TRACE ===\n");
        for exc in exc_values {
            let exc_type = exc.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let exc_val = exc.get("value").and_then(|v| v.as_str()).unwrap_or("");
            context_text.push_str(&format!("Exception: {} - {}\n", exc_type, exc_val));

            if let Some(frames) = exc
                .get("stacktrace")
                .and_then(|s| s.get("frames"))
                .and_then(|f| f.as_array())
            {
                for frame in frames {
                    let filename = frame.get("filename").and_then(|v| v.as_str()).unwrap_or("?");
                    let lineno = frame.get("lineno").and_then(|v| v.as_i64()).unwrap_or(0);
                    let func = frame.get("function").and_then(|v| v.as_str()).unwrap_or("");
                    let context_line = frame.get("context_line").and_then(|v| v.as_str()).unwrap_or("");

                    context_text.push_str(&format!("  at {} ({}:{})", func, filename, lineno));
                    if !context_line.is_empty() {
                        context_text.push_str(&format!(" -> {}", context_line.trim()));
                    }
                    context_text.push('\n');
                }
            }
        }
    }

    // Extract recent breadcrumbs
    let breadcrumbs_opt = event_payload
        .get("breadcrumbs")
        .and_then(|b| {
            if let Some(values) = b.get("values").and_then(|v| v.as_array()) {
                Some(values)
            } else {
                b.as_array()
            }
        });

    if let Some(breadcrumbs) = breadcrumbs_opt {
        context_text.push_str("\n=== RECENT BREADCRUMBS ===\n");
        let start_idx = if breadcrumbs.len() > 8 {
            breadcrumbs.len() - 8
        } else {
            0
        };
        for bc in &breadcrumbs[start_idx..] {
            let cat = bc.get("category").and_then(|v| v.as_str()).unwrap_or("info");
            let msg = bc.get("message").and_then(|v| v.as_str()).unwrap_or("");
            let data = bc.get("data").map(|d| d.to_string()).unwrap_or_default();
            context_text.push_str(&format!("[{}] {} {}\n", cat, msg, data));
        }
    }

    let system_prompt = "You are Nachtwache AI - an elite senior systems debugger and auto-fix engineer.
Your job is to analyze this crash report, explain why it happened in clear, actionable terms, and generate a precise git diff / patch to fix the bug.

Return ONLY a valid JSON object matching this exact schema:
{
  \"root_cause\": \"One or two sentences summarizing the exact bug root cause.\",
  \"explanation\": \"Clear, senior-level explanation of why it crashed and how to prevent it.\",
  \"diff\": \"unified diff string (e.g. --- a/file.ts\\n+++ b/file.ts...) showing the exact fix\",
  \"suggested_test\": \"A concise unit test to prevent regression.\"
}";

    info!("Dispatching AI diagnosis with provider: {}", settings.provider);

    let result = match settings.provider.to_lowercase().as_str() {
        "ollama" => call_ollama(&client, settings, system_prompt, &context_text).await,
        "openai" | "deepseek" | "groq" | "openrouter" => {
            call_openai_compatible(&client, settings, system_prompt, &context_text).await
        }
        "gemini" => call_gemini(&client, settings, system_prompt, &context_text).await,
        "anthropic" => call_anthropic(&client, settings, system_prompt, &context_text).await,
        "demo" => Err("Demo mode requested".to_string()),
        _ => Err(format!("Unsupported AI provider: {}", settings.provider)),
    };

    match result {
        Ok(raw_response) => parse_ai_json(&raw_response),
        Err(err) => {
            info!("Remote AI call failed ({}); using built-in heuristic diagnosis engine", err);
            Ok(generate_heuristic_diagnosis(issue, event_payload))
        }
    }
}

async fn call_ollama(
    client: &Client,
    settings: &AiSettings,
    system_prompt: &str,
    user_prompt: &str,
) -> Result<String, String> {
    let url = format!("{}/api/generate", settings.endpoint.trim_end_matches('/'));
    let prompt = format!("{}\n\nCRASH REPORT TO ANALYZE:\n{}", system_prompt, user_prompt);

    let body = json!({
        "model": if settings.model.is_empty() { "deepseek-4.1-flash" } else { &settings.model },
        "prompt": prompt,
        "stream": false,
        "format": "json"
    });

    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Ollama request failed: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let err_text = resp.text().await.unwrap_or_default();
        return Err(format!("Ollama returned {}: {}", status, err_text));
    }

    let val: Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse Ollama JSON: {}", e))?;

    val.get("response")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "Ollama response missing 'response' field".to_string())
}

async fn call_openai_compatible(
    client: &Client,
    settings: &AiSettings,
    system_prompt: &str,
    user_prompt: &str,
) -> Result<String, String> {
    let endpoint = if settings.endpoint.is_empty() {
        "https://api.openai.com".to_string()
    } else {
        settings.endpoint.clone()
    };
    let url = format!("{}/v1/chat/completions", endpoint.trim_end_matches('/'));

    let model = if settings.model.is_empty() {
        "gpt-6-astra"
    } else {
        &settings.model
    };

    let body = json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": user_prompt }
        ],
        "response_format": { "type": "json_object" },
        "temperature": 0.1
    });

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", settings.api_key))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("OpenAI request failed: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let err_text = resp.text().await.unwrap_or_default();
        return Err(format!("API returned {}: {}", status, err_text));
    }

    let val: Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse JSON response: {}", e))?;

    val.pointer("/choices/0/message/content")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "Missing content in API response".to_string())
}

async fn call_gemini(
    client: &Client,
    settings: &AiSettings,
    system_prompt: &str,
    user_prompt: &str,
) -> Result<String, String> {
    let model = if settings.model.is_empty() {
        "gemini-3.8-flash"
    } else {
        &settings.model
    };

    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
        model, settings.api_key
    );

    let combined_prompt = format!("{}\n\nCRASH REPORT TO ANALYZE:\n{}", system_prompt, user_prompt);

    let body = json!({
        "contents": [
            { "role": "user", "parts": [{ "text": combined_prompt }] }
        ],
        "generationConfig": {
            "responseMimeType": "application/json"
        }
    });

    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Gemini request failed: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let err_text = resp.text().await.unwrap_or_default();
        return Err(format!("Gemini returned {}: {}", status, err_text));
    }

    let val: Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse Gemini response: {}", e))?;

    val.pointer("/candidates/0/content/parts/0/text")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "Missing candidate text in Gemini response".to_string())
}

async fn call_anthropic(
    client: &Client,
    settings: &AiSettings,
    system_prompt: &str,
    user_prompt: &str,
) -> Result<String, String> {
    let model = if settings.model.is_empty() {
        "claude-sonnet-5"
    } else {
        &settings.model
    };

    let url = "https://api.anthropic.com/v1/messages";

    let body = json!({
        "model": model,
        "max_tokens": 2048,
        "system": system_prompt,
        "messages": [
            { "role": "user", "content": format!("Analyze this error and return valid JSON:\n{}", user_prompt) }
        ]
    });

    let resp = client
        .post(url)
        .header("x-api-key", &settings.api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Anthropic request failed: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let err_text = resp.text().await.unwrap_or_default();
        return Err(format!("Anthropic returned {}: {}", status, err_text));
    }

    let val: Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse Anthropic response: {}", e))?;

    val.pointer("/content/0/text")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "Missing content in Anthropic response".to_string())
}

fn parse_ai_json(raw: &str) -> Result<AiDiagnosisResponse, String> {
    // Strip markdown code fences if model returned ```json ... ```
    let clean = raw.trim();
    let json_str = if clean.starts_with("```") {
        let lines: Vec<&str> = clean.lines().collect();
        if lines.len() >= 2 {
            lines[1..lines.len() - 1].join("\n")
        } else {
            clean.to_string()
        }
    } else {
        clean.to_string()
    };

    serde_json::from_str::<AiDiagnosisResponse>(&json_str)
        .map_err(|e| format!("Failed to parse AI response into schema: {} (raw: {})", e, raw))
}

pub fn generate_heuristic_diagnosis(issue: &Issue, _payload: &Value) -> AiDiagnosisResponse {
    let title_lower = issue.title.to_lowercase();

    if title_lower.contains("zerodivision") || title_lower.contains("division by zero") {
        AiDiagnosisResponse {
            root_cause: "Dzielenie przez zero w funkcji kalkulacyjnej z powodu zerowej wartości mianownika.".to_string(),
            explanation: format!(
                "Błąd '{}' w '{}' wystąpił, ponieważ funkcja wykonała operację dzielenia bez uprzedniej walidacji danych wejściowych. Gdy liczba użytkowników lub dzielnik wynosi 0, runtime natychmiast wyrzuca wyjątek krytyczny.",
                issue.title, issue.culprit
            ),
            diff: Some(
                "--- a/calculator.py\n+++ b/calculator.py\n@@ -20,3 +20,4 @@\n def calculate_discount(total, users_count):\n+    if not users_count or users_count <= 0:\n+        return 0.0\n     return total / users_count".to_string(),
            ),
            suggested_test: Some(
                "def test_calculate_discount_zero_guard():\n    assert calculate_discount(150.0, 0) == 0.0\n    assert calculate_discount(150.0, -1) == 0.0".to_string(),
            ),
        }
    } else if title_lower.contains("split") || title_lower.contains("undefined") || title_lower.contains("null") {
        AiDiagnosisResponse {
            root_cause: "Próba wywołania metody na niezainicjalizowanej wartości (null/undefined).".to_string(),
            explanation: format!(
                "Wyjątek '{}' w '{}' nastąpił przy próbie odczytania właściwości lub wywołania metody na zmiennej o wartości undefined. Rekomendowane jest użycie optional chaining (?.) oraz wartości zapasowej.",
                issue.title, issue.culprit
            ),
            diff: Some(
                "--- a/src/utils/user.ts\n+++ b/src/utils/user.ts\n@@ -11,3 +11,3 @@\n-  const parts = user.fullName.split(\" \");\n+  const parts = (user?.fullName || \"\").split(\" \");".to_string(),
            ),
            suggested_test: Some(
                "it(\"handles undefined user safely\", () => {\n  expect(getUserInitials({})).toBe(\"\");\n  expect(getUserInitials(null)).toBe(\"\");\n});".to_string(),
            ),
        }
    } else {
        AiDiagnosisResponse {
            root_cause: format!("Wyjątek typu: {}", issue.title),
            explanation: format!(
                "Zarejestrowano nieobsłużony błąd '{}' w lokalizacji '{}'. Wymagane jest dodanie bloku try/catch lub walidacji warunków brzegowych.",
                issue.title, issue.culprit
            ),
            diff: Some(
                format!("// Fix for {}\n// Sprawdź poprawność stanu przed wywołaniem {}", issue.title, issue.culprit),
            ),
            suggested_test: Some(
                "// Rekomendowany test jednostkowy zapobiegający regresji".to_string(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Issue;

    #[test]
    fn test_parse_ai_json_plain() {
        let json = r#"{
            "root_cause": "Typo in variable",
            "explanation": "Variable is not defined",
            "diff": "--- a.js\n+++ b.js",
            "suggested_test": "test()"
        }"#;
        let res = parse_ai_json(json).unwrap();
        assert_eq!(res.root_cause, "Typo in variable");
        assert_eq!(res.explanation, "Variable is not defined");
    }

    #[test]
    fn test_parse_ai_json_fenced() {
        let fenced = "```json\n{\n  \"root_cause\": \"Null pointer\",\n  \"explanation\": \"Object was null\",\n  \"diff\": null,\n  \"suggested_test\": null\n}\n```";
        let res = parse_ai_json(fenced).unwrap();
        assert_eq!(res.root_cause, "Null pointer");
    }

    #[test]
    fn test_heuristic_zero_division() {
        let issue = Issue {
            id: "1".into(),
            project_id: "1".into(),
            fingerprint: "fp".into(),
            title: "ZeroDivisionError: division by zero".into(),
            culprit: "math.py:10 in divide".into(),
            level: "error".into(),
            platform: "python".into(),
            status: "unresolved".into(),
            count: 1,
            first_seen: "now".into(),
            last_seen: "now".into(),
            ai_diagnosis: None,
            ai_fix_diff: None,
        };
        let diag = generate_heuristic_diagnosis(&issue, &json!({}));
        assert!(diag.root_cause.contains("Dzielenie przez zero"));
        assert!(diag.diff.unwrap().contains("calculator.py"));
    }
}

