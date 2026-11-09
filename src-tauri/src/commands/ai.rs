//! AI assist commands (SPEC §7.7). Nothing is sent to the provider unless the user clicks.

use tauri::State;

use crate::db::models::*;
use crate::db::repo::{courses, lessons, restructure, transcripts};
use crate::error::{AppError, AppResult};
use crate::services::ai::{self, LessonInfo};
use crate::services::secrets;
use crate::state::AppState;

fn ai_settings(state: &AppState) -> AppResult<AiSettings> {
    ai::ai_settings(state)
}

#[tauri::command]
#[specta::specta]
pub async fn get_ai_status(state: State<'_, AppState>) -> AppResult<AiStatus> {
    Ok(AiStatus {
        settings: ai_settings(&state)?,
        has_key: secrets::get(ai::KEY_NAME).ok().flatten().is_some(),
    })
}

#[tauri::command]
#[specta::specta]
pub async fn set_ai_settings(
    state: State<'_, AppState>,
    settings: AiSettings,
) -> AppResult<AiStatus> {
    if !ai::MODELS.iter().any(|(id, _)| *id == settings.model) {
        return Err(AppError::invalid("unknown model"));
    }
    crate::db::repo::settings::set(&*state.db()?, ai::SETTINGS_KEY, &settings)?;
    get_ai_status(state).await
}

#[tauri::command]
#[specta::specta]
pub async fn set_ai_key(state: State<'_, AppState>, key: Option<String>) -> AppResult<AiStatus> {
    match key.as_deref().map(str::trim).filter(|k| !k.is_empty()) {
        Some(k) => secrets::set(ai::KEY_NAME, k)?,
        None => secrets::delete(ai::KEY_NAME)?,
    }
    get_ai_status(state).await
}

/// Gathers what the model sees about each video lesson (no network).
fn lesson_infos(
    state: &AppState,
    course_id: &str,
    with_transcripts: bool,
) -> AppResult<(Course, Vec<LessonInfo>, Vec<String>)> {
    let conn = state.db()?;
    let course = courses::get_course(&conn, course_id)?;
    let all = lessons::list_course_lessons(&conn, course_id)?;
    let ids = all.iter().map(|l| l.id.clone()).collect();
    let mut infos = Vec::with_capacity(all.len());
    for l in all {
        let chapters = transcripts::get_chapters(&conn, &l.id)?
            .into_iter()
            .map(|c| c.title)
            .take(12)
            .collect();
        let transcript_excerpt = if with_transcripts {
            let t = transcripts::get_transcript(&conn, &l.id)?;
            let text: Vec<String> = t
                .cues
                .iter()
                .take_while(|c| c.start < 120.0)
                .map(|c| c.text.clone())
                .collect();
            (!text.is_empty()).then(|| text.join(" "))
        } else {
            None
        };
        infos.push(LessonInfo {
            id: l.id,
            title: l.title,
            duration_sec: l.duration_sec,
            description: l.description,
            chapters,
            transcript_excerpt,
        });
    }
    Ok((course, infos, ids))
}

fn restructure_call<'a>(
    course: &Course,
    infos: &[LessonInfo],
    extra: Option<String>,
    course_id: &'a str,
) -> ai::Call<'a> {
    let mut prompt = ai::build_prompt(&course.title, course.description.as_deref(), infos);
    if let Some(x) = extra {
        prompt.push_str("\n\n");
        prompt.push_str(&x);
    }
    ai::Call {
        feature: "restructure",
        system: ai::RESTRUCTURE_SYSTEM,
        prompt,
        schema: ai::restructure_schema(),
        expected_output: ai::estimate_output_tokens(infos.len()),
        course_id: Some(course_id),
        lesson_id: None,
    }
}

/// Local estimate (no network) of a call's cost, plus month-to-date spend against the budget.
pub fn to_estimate(state: &AppState, call: &ai::Call<'_>, items: usize) -> AppResult<AiEstimate> {
    let (model, input, output, cost) = ai::estimate(state, call)?;
    let s = ai::ai_settings(state)?;
    let spent = crate::db::repo::ai_usage::month_to_date(&*state.db()?, chrono::Utc::now())?;
    Ok(AiEstimate {
        items: items as i32,
        input_tokens: input,
        output_tokens: output,
        cost_usd: cost,
        model,
        month_spent_usd: spent,
        limit_usd: s.monthly_limit_usd,
    })
}

/// Local estimate of tokens and cost for a restructure (nothing is sent).
#[tauri::command]
#[specta::specta]
pub async fn estimate_restructure(
    state: State<'_, AppState>,
    course_id: String,
    with_transcripts: bool,
) -> AppResult<AiEstimate> {
    let (course, infos, _) = lesson_infos(&state, &course_id, with_transcripts)?;
    let extra = crate::services::ai_context::course_outline_context(&*state.db()?, &course_id)?;
    to_estimate(
        &state,
        &restructure_call(&course, &infos, extra, &course_id),
        infos.len(),
    )
}

/// Sends the course outline to Claude and returns a validated proposal (not applied yet).
#[tauri::command]
#[specta::specta]
pub async fn propose_restructure(
    state: State<'_, AppState>,
    course_id: String,
    with_transcripts: bool,
) -> AppResult<RestructureProposal> {
    let (course, infos, ids) = lesson_infos(&state, &course_id, with_transcripts)?;
    if infos.len() < 2 {
        return Err(AppError::invalid(
            "a course needs at least two lessons to restructure",
        ));
    }
    let extra = crate::services::ai_context::course_outline_context(&*state.db()?, &course_id)?;
    let (v, used) = ai::run(&state, restructure_call(&course, &infos, extra, &course_id)).await?;
    let p: ai::Proposal = serde_json::from_value(v)
        .map_err(|e| AppError::internal(format!("the AI returned an unexpected format: {e}")))?;
    let (modules, unsorted, dropped) = restructure::normalize(p.modules, &ids);
    Ok(RestructureProposal {
        modules,
        rationale: p.rationale,
        unsorted,
        dropped,
        cost_usd: used.cost_usd,
        model: used.model,
        input_tokens: used.input_tokens,
        output_tokens: used.output_tokens,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn get_ai_usage(state: State<'_, AppState>) -> AppResult<AiUsageSummary> {
    let limit = ai::ai_settings(&state)?.monthly_limit_usd;
    crate::db::repo::ai_usage::summary(&*state.db()?, chrono::Utc::now(), limit)
}

#[tauri::command]
#[specta::specta]
pub async fn clear_ai_usage(state: State<'_, AppState>) -> AppResult<()> {
    crate::db::repo::ai_usage::clear(&*state.db()?)
}

/// Applies a (possibly edited) structure; the previous one is kept for undo.
#[tauri::command]
#[specta::specta]
pub async fn apply_course_structure(
    state: State<'_, AppState>,
    course_id: String,
    modules: Vec<StructureModule>,
) -> AppResult<()> {
    restructure::apply(&*state.db()?, &course_id, &modules, true)
}

#[tauri::command]
#[specta::specta]
pub async fn undo_restructure(state: State<'_, AppState>, course_id: String) -> AppResult<()> {
    restructure::undo(&*state.db()?, &course_id)
}

#[tauri::command]
#[specta::specta]
pub async fn can_undo_restructure(
    state: State<'_, AppState>,
    course_id: String,
) -> AppResult<bool> {
    restructure::has_undo(&*state.db()?, &course_id)
}
