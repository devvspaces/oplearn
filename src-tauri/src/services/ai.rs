//! Optional AI assist (SPEC §7.7) via the Claude Messages API. Off by default; the key lives in
//! the OS keychain; nothing is sent without an explicit user action, and the cost is estimated
//! locally (no network) before sending.
//!
//! Raw HTTP because there is no official Anthropic SDK for Rust.

use serde::Deserialize;
use serde_json::{Value, json};

use crate::db::models::StructureModule;
use crate::db::repo::{ai_usage, settings};
use crate::error::{AppError, AppResult, ErrorCode};
use crate::state::AppState;

pub const API_URL: &str = "https://api.anthropic.com/v1/messages";
pub const API_VERSION: &str = "2026-10-21";
pub const KEY_NAME: &str = "anthropic_api_key";
pub const SETTINGS_KEY: &str = "ai";
/// Server-side refusal fallback ("default" routing by refusal category).
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

pub const MODELS: &[(&str, &str)] = &[
    ("claude-opus-5-5", "Claude Opus 5.5"),
    ("claude-sonnet-5-5", "Claude Sonnet 5.5"),
];

/// USD per million (input, output) tokens.
pub fn price(model: &str) -> (f64, f64) {
    match model {
        "claude-sonnet-5-5" => (2.0, 10.0),
        _ => (4.0, 20.0),
    }
}

pub fn cost(model: &str, input: i32, output: i32) -> f64 {
    let (i, o) = price(model);
    (f64::from(input) * i + f64::from(output) * o) / 1e6
}

/// Rough token count for an estimate shown before sending (~3.5 characters per token, rounded
/// up). The real count comes back in the response's `usage`.
pub fn estimate_tokens(text: &str) -> i32 {
    ((text.chars().count() as f64) / 3.5).ceil() as i32
}

pub struct LessonInfo {
    pub id: String,
    pub title: String,
    pub duration_sec: Option<i32>,
    pub description: Option<String>,
    pub chapters: Vec<String>,
    pub transcript_excerpt: Option<String>,
}

pub const RESTRUCTURE_SYSTEM: &str = "You organise lecture videos into a course outline for a self-learner. \
Group lessons into coherent modules (typically 3-12, each 2-10 lessons) and order modules and the lessons \
inside them so prerequisites come first. Keep the creator's original order where it is already sensible: \
numbered lectures usually are. Module titles are short and descriptive (e.g. \"Week 2: Replication\" or \
\"Consensus with Raft\"), with a one-sentence summary. Use every lesson id exactly once and only ids from the input. \
In the rationale, explain the main decisions in 2-4 sentences.";

fn truncate(s: &str, n: usize) -> String {
    let t: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        format!("{t}…")
    } else {
        t
    }
}

pub fn build_prompt(
    course_title: &str,
    course_description: Option<&str>,
    lessons: &[LessonInfo],
) -> String {
    let mut s = format!("Course: {course_title}\n");
    if let Some(d) = course_description.filter(|d| !d.trim().is_empty()) {
        s.push_str(&format!("Description: {}\n", truncate(d.trim(), 600)));
    }
    s.push_str(&format!(
        "\n{} lessons in their current order:\n",
        lessons.len()
    ));
    for (i, l) in lessons.iter().enumerate() {
        s.push_str(&format!("\n[{}] id={} | {}", i + 1, l.id, l.title));
        if let Some(d) = l.duration_sec {
            s.push_str(&format!(" | {} min", (d + 30) / 60));
        }
        if let Some(d) = l.description.as_deref().filter(|d| !d.trim().is_empty()) {
            s.push_str(&format!(
                "\n  description: {}",
                truncate(&d.replace('\n', " "), 300)
            ));
        }
        if !l.chapters.is_empty() {
            s.push_str(&format!(
                "\n  chapters: {}",
                truncate(&l.chapters.join("; "), 300)
            ));
        }
        if let Some(t) = &l.transcript_excerpt {
            s.push_str(&format!("\n  opening: {}", truncate(t, 700)));
        }
    }
    s
}

pub fn restructure_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["modules", "rationale"],
        "properties": {
            "modules": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["title", "summary", "lessonIds"],
                    "properties": {
                        "title": {"type": "string"},
                        "summary": {"type": "string"},
                        "lessonIds": {"type": "array", "items": {"type": "string"}}
                    }
                }
            },
            "rationale": {"type": "string"}
        }
    })
}

/// Expected output tokens for a restructure: ~30 per lesson plus module text and thinking headroom.
pub fn estimate_output_tokens(lessons: usize) -> i32 {
    600 + 30 * lessons as i32 + 1500
}

/// One structured-output request to the Messages API.
pub struct Call<'a> {
    pub feature: &'a str,
    pub system: &'a str,
    pub prompt: String,
    pub schema: Value,
    /// Expected output tokens (for the pre-flight cost estimate and `max_tokens`).
    pub expected_output: i32,
    pub course_id: Option<&'a str>,
    pub lesson_id: Option<&'a str>,
}

impl Call<'_> {
    pub fn estimate_input(&self) -> i32 {
        estimate_tokens(&self.prompt) + estimate_tokens(self.system) + 250
    }
}

pub fn request_body(
    model: &str,
    system: &str,
    prompt: &str,
    schema: &Value,
    expected_output: i32,
) -> Value {
    json!({
        "model": model,
        "max_tokens": (expected_output * 4).clamp(8000, 32000),
        "system": system,
        "messages": [{"role": "user", "content": prompt}],
        "output_config": {
            "effort": "medium",
            "format": {"type": "json_schema", "schema": schema}
        },
        "fallbacks": "default"
    })
}

#[derive(Debug, Deserialize, Default)]
struct Usage {
    #[serde(default)]
    input_tokens: i32,
    #[serde(default)]
    output_tokens: i32,
}

#[derive(Debug, Deserialize)]
struct ApiResponse {
    #[serde(default)]
    content: Vec<Value>,
    stop_reason: Option<String>,
    usage: Option<Usage>,
    model: Option<String>,
}

/// What a call cost, as reported by the API.
#[derive(Debug, Clone)]
pub struct Used {
    pub model: String,
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub cost_usd: f64,
}

/// Parses a Messages API response into the structured JSON plus usage (pure; unit-tested).
/// Usage is returned even when the content is unusable, so failed calls are still billed in the ledger.
pub fn parse_structured(body: &str, requested_model: &str) -> (AppResult<Value>, Used) {
    let r: ApiResponse = match serde_json::from_str(body) {
        Ok(r) => r,
        Err(e) => {
            let used = Used {
                model: requested_model.into(),
                input_tokens: 0,
                output_tokens: 0,
                cost_usd: 0.0,
            };
            return (
                Err(AppError::internal(format!("unexpected AI response: {e}"))),
                used,
            );
        }
    };
    let u = r.usage.unwrap_or_default();
    let model = r.model.unwrap_or_else(|| requested_model.to_string());
    let used = Used {
        cost_usd: cost(&model, u.input_tokens, u.output_tokens),
        model,
        input_tokens: u.input_tokens,
        output_tokens: u.output_tokens,
    };
    let result = match r.stop_reason.as_deref() {
        Some("refusal") => Err(AppError::new(
            ErrorCode::Invalid,
            "The AI declined this request.",
        )),
        Some("max_tokens") => Err(AppError::internal(
            "The AI response was cut off. Try again with a smaller scope.",
        )),
        _ => {
            let text: String = r
                .content
                .iter()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .collect();
            serde_json::from_str::<Value>(text.trim()).map_err(|e| {
                AppError::internal(format!("the AI returned an unexpected format: {e}"))
            })
        }
    };
    (result, used)
}

#[derive(Debug, Deserialize)]
struct ErrBody {
    error: ErrInner,
}
#[derive(Debug, Deserialize)]
struct ErrInner {
    message: String,
}

pub fn ai_settings(state: &AppState) -> AppResult<crate::db::models::AiSettings> {
    Ok(
        settings::get::<crate::db::models::AiSettings>(&*state.db()?, SETTINGS_KEY)?
            .unwrap_or_default(),
    )
}

/// Monthly budget rule: refuse when month-to-date spend plus this call's estimate exceeds the limit.
pub fn check_budget(spent: f64, estimate: f64, limit: Option<f64>) -> AppResult<()> {
    match limit {
        Some(l) if l >= 0.0 && spent + estimate > l => Err(AppError::new(
            ErrorCode::Quota,
            format!(
                "Monthly AI budget reached (${spent:.2} of ${l:.2} spent). Raise it in Settings → AI."
            ),
        )),
        _ => Ok(()),
    }
}

/// Estimated cost for a call with the current model, before sending (no network).
pub fn estimate(state: &AppState, call: &Call<'_>) -> AppResult<(String, i32, i32, f64)> {
    let s = ai_settings(state)?;
    let input = call.estimate_input();
    Ok((
        s.model.clone(),
        input,
        call.expected_output,
        cost(&s.model, input, call.expected_output),
    ))
}

/// Runs a structured call: checks that AI is enabled, the key exists and the budget allows it,
/// sends the request, and records usage (also for failed calls).
pub async fn run(state: &AppState, call: Call<'_>) -> AppResult<(Value, Used)> {
    let s = ai_settings(state)?;
    if !s.enabled {
        return Err(AppError::invalid(
            "turn on AI features in Settings → AI first",
        ));
    }
    let key = crate::services::secrets::get(KEY_NAME)?
        .ok_or_else(|| AppError::invalid("add your Anthropic API key in Settings → AI first"))?;
    let est = cost(&s.model, call.estimate_input(), call.expected_output);
    {
        let conn = state.db()?;
        check_budget(
            ai_usage::month_to_date(&conn, chrono::Utc::now())?,
            est,
            s.monthly_limit_usd,
        )?;
    }
    let body = request_body(
        &s.model,
        call.system,
        &call.prompt,
        &call.schema,
        call.expected_output,
    );
    let resp = state
        .http
        .post(API_URL)
        .header("x-api-key", key)
        .header("anthropic-version", API_VERSION)
        .header("anthropic-beta", FALLBACK_BETA)
        .header("content-type", "application/json")
        .timeout(std::time::Duration::from_secs(300))
        .json(&body)
        .send()
        .await?;
    let status = resp.status();
    let text = resp.text().await?;
    if !status.is_success() {
        let msg = serde_json::from_str::<ErrBody>(&text)
            .map(|e| e.error.message)
            .unwrap_or_else(|_| format!("HTTP {status}"));
        let code = match status.as_u16() {
            401 | 403 => ErrorCode::Invalid,
            429 => ErrorCode::Quota,
            _ => ErrorCode::Http,
        };
        let hint = if status.as_u16() == 401 {
            " Check your Anthropic API key in Settings → AI."
        } else {
            ""
        };
        return Err(AppError::new(code, format!("Claude API: {msg}.{hint}")));
    }
    let (result, used) = parse_structured(&text, &s.model);
    ai_usage::record(
        &*state.db()?,
        call.feature,
        &used,
        call.course_id,
        call.lesson_id,
        result.is_ok(),
    )?;
    Ok((result?, used))
}

#[derive(Debug, Deserialize)]
pub struct Proposal {
    pub modules: Vec<StructureModule>,
    pub rationale: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_structured_response_and_rejects_refusal() {
        let body = json!({
            "model": "claude-opus-5-5",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 1200, "output_tokens": 300},
            "content": [
                {"type": "thinking", "thinking": ""},
                {"type": "text", "text": "{\"modules\":[{\"title\":\"Basics\",\"summary\":\"Intro\",\"lessonIds\":[\"a\",\"b\"]}],\"rationale\":\"Kept order.\"}"}
            ]
        })
        .to_string();
        let (v, used) = parse_structured(&body, "x");
        let p: Proposal = serde_json::from_value(v.unwrap()).unwrap();
        assert_eq!(p.modules[0].lesson_ids, vec!["a", "b"]);
        assert_eq!((used.input_tokens, used.output_tokens), (1200, 300));
        assert!((used.cost_usd - (1200.0 * 4.0 + 300.0 * 20.0) / 1e6).abs() < 1e-12);
        let refused = json!({"stop_reason": "refusal", "content": [], "usage": {"input_tokens": 50, "output_tokens": 0}}).to_string();
        let (r, used) = parse_structured(&refused, "claude-opus-5-5");
        assert!(r.is_err());
        assert_eq!(used.input_tokens, 50, "refusals are still logged");
    }

    #[test]
    fn prompt_and_body_shape() {
        let p = build_prompt(
            "6.824",
            Some("Distributed systems"),
            &[LessonInfo {
                id: "L1".into(),
                title: "Intro".into(),
                duration_sec: Some(4800),
                description: None,
                chapters: vec!["MapReduce".into()],
                transcript_excerpt: None,
            }],
        );
        assert!(p.contains("id=L1 | Intro | 80 min"));
        assert!(p.contains("chapters: MapReduce"));
        let b = request_body(
            "claude-opus-5-5",
            RESTRUCTURE_SYSTEM,
            &p,
            &restructure_schema(),
            2000,
        );
        assert_eq!(b["output_config"]["format"]["type"], "json_schema");
        assert_eq!(b["fallbacks"], "default");
        assert!(b.get("thinking").is_none() && b.get("temperature").is_none());
        assert!((cost("claude-opus-5-5", 1_000_000, 0) - 4.0).abs() < 1e-9);
    }

    #[test]
    fn budget_rule() {
        assert!(check_budget(1.0, 0.5, None).is_ok());
        assert!(check_budget(1.0, 0.5, Some(2.0)).is_ok());
        assert!(check_budget(1.9, 0.2, Some(2.0)).is_err());
        assert!(check_budget(0.0, 0.02, Some(0.01)).is_err());
    }
}
