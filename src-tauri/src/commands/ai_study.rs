//! AI flashcards and quiz banks. Generation returns drafts; nothing is saved until the user
//! picks which ones to keep. Estimates are local (no network).

use chrono::Utc;
use tauri::State;

use crate::commands::ai::to_estimate;
use crate::db::models::*;
use crate::db::repo::{flashcards, lessons, quiz, study};
use crate::error::{AppError, AppResult};
use crate::services::ai::{self, Call};
use crate::services::ai_context::{self, Context, LessonMaterial};
use crate::services::ai_study;
use crate::state::AppState;

/// Characters of material per request (~45k / ~70k input tokens).
const LESSON_CHARS: usize = 160_000;
const WIDE_CHARS: usize = 240_000;
/// Lessons sampled for course-wide quizzes.
const MAX_LESSONS: usize = 15;

fn has_content(m: &LessonMaterial) -> bool {
    !m.cues.is_empty() || !m.notes.is_empty() || !m.pdfs.is_empty()
}

/// Loads the material for a scope and returns (context, course id, single lesson id).
fn load_scope(
    state: &AppState,
    scope: QuizScope,
    scope_id: &str,
    include_notes: bool,
) -> AppResult<(Context, String, Option<String>)> {
    let conn = state.db()?;
    let (course_id, ids, lesson): (String, Vec<String>, Option<String>) = match scope {
        QuizScope::Lesson => (
            lessons::course_id_of_lesson(&conn, scope_id)?,
            vec![scope_id.to_string()],
            Some(scope_id.to_string()),
        ),
        QuizScope::Module => {
            let m = crate::db::repo::modules::get_module(&conn, scope_id)?;
            let ids = lessons::list_course_lessons(&conn, &m.course_id)?
                .into_iter()
                .filter(|l| l.module_id == scope_id)
                .map(|l| l.id)
                .collect();
            (m.course_id, ids, None)
        }
        QuizScope::Course => (
            scope_id.to_string(),
            lessons::list_course_lessons(&conn, scope_id)?
                .into_iter()
                .map(|l| l.id)
                .collect(),
            None,
        ),
    };
    let mut mats: Vec<LessonMaterial> = ids
        .iter()
        .map(|id| ai_context::load_lesson(&conn, id, include_notes))
        .collect::<AppResult<Vec<_>>>()?
        .into_iter()
        .filter(has_content)
        .collect();
    if mats.is_empty() {
        return Err(AppError::invalid(
            "there's no transcript, notes or lecture notes to work from yet; fetch the transcript first",
        ));
    }
    if mats.len() > MAX_LESSONS {
        // Spread the sample across the course rather than taking the first N.
        let step = mats.len() as f64 / MAX_LESSONS as f64;
        mats = (0..MAX_LESSONS)
            .map(|i| mats[(i as f64 * step) as usize].clone())
            .collect();
    }
    let chars = if mats.len() == 1 {
        LESSON_CHARS
    } else {
        WIDE_CHARS
    };
    Ok((ai_context::build(&mats, chars), course_id, lesson))
}

fn cards_call<'a>(ctx: &Context, count: u32, lesson_id: &'a str) -> Call<'a> {
    Call {
        feature: "flashcards",
        system: ai_study::CARDS_SYSTEM,
        prompt: ai_study::cards_prompt(ctx, count),
        schema: ai_study::cards_schema(),
        expected_output: ai_study::expected_cards_output(count),
        course_id: None,
        lesson_id: Some(lesson_id),
    }
}

fn quiz_call<'a>(
    ctx: &Context,
    count: u32,
    mcq_share: f64,
    course_id: &'a str,
    lesson_id: Option<&'a str>,
) -> Call<'a> {
    Call {
        feature: "quiz",
        system: ai_study::QUIZ_SYSTEM,
        prompt: ai_study::quiz_prompt(ctx, count, mcq_share),
        schema: ai_study::quiz_schema(),
        expected_output: ai_study::expected_quiz_output(count),
        course_id: Some(course_id),
        lesson_id,
    }
}

#[tauri::command]
#[specta::specta]
pub async fn estimate_flashcards(
    state: State<'_, AppState>,
    lesson_id: String,
    count: i32,
    include_notes: bool,
) -> AppResult<AiEstimate> {
    let (ctx, _, _) = load_scope(&state, QuizScope::Lesson, &lesson_id, include_notes)?;
    to_estimate(
        &state,
        &cards_call(&ctx, count.clamp(1, 50) as u32, &lesson_id),
        count as usize,
    )
}

#[tauri::command]
#[specta::specta]
pub async fn generate_flashcards(
    state: State<'_, AppState>,
    lesson_id: String,
    count: i32,
    include_notes: bool,
) -> AppResult<Vec<FlashcardDraft>> {
    let (ctx, _, _) = load_scope(&state, QuizScope::Lesson, &lesson_id, include_notes)?;
    let (v, _) = ai::run(
        &state,
        cards_call(&ctx, count.clamp(1, 50) as u32, &lesson_id),
    )
    .await?;
    let drafts = ai_study::to_card_drafts(v, &ctx);
    if drafts.is_empty() {
        return Err(AppError::internal(
            "the AI didn't return any usable cards; try again",
        ));
    }
    Ok(drafts)
}

/// Saves chosen AI drafts as cards (marked AI-generated).
#[tauri::command]
#[specta::specta]
pub async fn save_flashcard_drafts(
    state: State<'_, AppState>,
    lesson_id: String,
    drafts: Vec<FlashcardDraft>,
) -> AppResult<i32> {
    let conn = state.db()?;
    let mut n = 0;
    for d in drafts {
        let card = NewFlashcard {
            lesson_id: Some(lesson_id.clone()),
            source_sec: d.source_sec,
            source_material_id: d.source_material_id,
            source_page: d.source_page,
            front_md: d.front_md,
            back_md: d.back_md,
        };
        if flashcards::create_with(&conn, &card, true).is_ok() {
            n += 1;
        }
    }
    Ok(n)
}

#[tauri::command]
#[specta::specta]
pub async fn estimate_quiz(
    state: State<'_, AppState>,
    scope: QuizScope,
    scope_id: String,
    count: i32,
    mcq_share: f64,
) -> AppResult<AiEstimate> {
    let (ctx, course_id, lesson) = load_scope(&state, scope, &scope_id, true)?;
    to_estimate(
        &state,
        &quiz_call(
            &ctx,
            count.clamp(1, 40) as u32,
            mcq_share,
            &course_id,
            lesson.as_deref(),
        ),
        count as usize,
    )
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct QuizDrafts {
    pub course_id: String,
    pub drafts: Vec<QuizDraft>,
    /// Questions the model returned that failed validation.
    pub dropped: i32,
}

#[tauri::command]
#[specta::specta]
pub async fn generate_quiz(
    state: State<'_, AppState>,
    scope: QuizScope,
    scope_id: String,
    count: i32,
    mcq_share: f64,
) -> AppResult<QuizDrafts> {
    let (ctx, course_id, lesson) = load_scope(&state, scope, &scope_id, true)?;
    let (v, _) = ai::run(
        &state,
        quiz_call(
            &ctx,
            count.clamp(1, 40) as u32,
            mcq_share,
            &course_id,
            lesson.as_deref(),
        ),
    )
    .await?;
    let (drafts, dropped) = ai_study::to_quiz_drafts(v, &ctx);
    if drafts.is_empty() {
        return Err(AppError::internal(
            "the AI didn't return any usable questions; try again",
        ));
    }
    Ok(QuizDrafts {
        course_id,
        drafts,
        dropped,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn save_quiz_questions(
    state: State<'_, AppState>,
    course_id: String,
    drafts: Vec<QuizDraft>,
    ai_generated: bool,
) -> AppResult<i32> {
    quiz::save(&*state.db()?, &course_id, &drafts, ai_generated)
}

#[tauri::command]
#[specta::specta]
pub async fn list_quiz(
    state: State<'_, AppState>,
    scope: QuizScope,
    scope_id: String,
) -> AppResult<Vec<QuizQuestion>> {
    quiz::list(&*state.db()?, scope, &scope_id)
}

/// Records an answer; `seconds` (≤ 3 min) count as review time.
#[tauri::command]
#[specta::specta]
pub async fn record_quiz_answer(
    state: State<'_, AppState>,
    id: String,
    correct: bool,
    seconds: f64,
) -> AppResult<()> {
    let conn = state.db()?;
    quiz::record_answer(&conn, &id, correct)?;
    let q = quiz::get(&conn, &id)?;
    study::record_activity(
        &conn,
        q.draft.lesson_id.as_deref(),
        ActivitySource::Review,
        seconds.min(180.0),
        Utc::now(),
    )
}

#[tauri::command]
#[specta::specta]
pub async fn delete_quiz_question(state: State<'_, AppState>, id: String) -> AppResult<()> {
    quiz::delete(&*state.db()?, &id)
}

// ---- AI study plan (uses syllabus/calendar context for provider courses) ----

/// lesson id → (title, duration).
type LessonInfoMap = std::collections::HashMap<String, (String, Option<i32>)>;

fn plan_prompt(
    state: &AppState,
    course_id: &str,
    target_date: Option<&str>,
) -> AppResult<(String, Vec<String>, LessonInfoMap)> {
    let conn = state.db()?;
    let course = crate::db::repo::courses::get_course(&conn, course_id)?;
    let detail = crate::db::repo::courses::get_course_detail(&conn, course_id)?;
    let s = study::get_settings(&conn)?;
    let mut open = vec![];
    let mut info = std::collections::HashMap::new();
    let mut p = format!("Course: {}\n", course.title);
    p.push_str(&format!(
        "Learner: daily goal {} min on {} ({} min/week). {}\nToday is {}.\n\nRemaining lessons in the current outline order:\n",
        s.daily_goal_min,
        s.study_days.join(", "),
        s.daily_goal_min as usize * s.study_days.len().max(1),
        target_date.map(|d| format!("Finish by {d}.")).unwrap_or_else(|| "No fixed deadline: plan a steady pace.".into()),
        chrono::Local::now().date_naive()
    ));
    for m in &detail.modules {
        p.push_str(&format!("\n## {}\n", m.module.title));
        for l in m.lessons.iter().filter(|l| l.completed_at.is_none()) {
            let kind = match l.lesson_type {
                LessonType::Video => "video".to_string(),
                _ => crate::db::repo::materials::list_for_lesson(&conn, &l.id)?
                    .first()
                    .map(|m| m.kind.clone())
                    .unwrap_or_else(|| "reading".into()),
            };
            p.push_str(&format!(
                "- id={} | {kind} | {}{}\n",
                l.id,
                l.title,
                l.duration_sec
                    .map(|d| format!(" | {} min", (d + 30) / 60))
                    .unwrap_or_default()
            ));
            open.push(l.id.clone());
            info.insert(l.id.clone(), (l.title.clone(), l.duration_sec));
        }
    }
    if let Some(extra) = ai_context::course_outline_context(&conn, course_id)? {
        p.push_str("\n\n");
        p.push_str(&extra);
    }
    if open.is_empty() {
        return Err(AppError::invalid(
            "every lesson in this course is already complete",
        ));
    }
    Ok((p, open, info))
}

fn plan_call<'a>(prompt: String, n: usize, course_id: &'a str) -> Call<'a> {
    Call {
        feature: "plan",
        system: ai_study::PLAN_SYSTEM,
        prompt,
        schema: ai_study::plan_schema(),
        expected_output: 1200 + 25 * n as i32,
        course_id: Some(course_id),
        lesson_id: None,
    }
}

#[tauri::command]
#[specta::specta]
pub async fn estimate_course_plan(
    state: State<'_, AppState>,
    course_id: String,
    target_date: Option<String>,
) -> AppResult<AiEstimate> {
    let (p, open, _) = plan_prompt(&state, &course_id, target_date.as_deref())?;
    to_estimate(&state, &plan_call(p, open.len(), &course_id), open.len())
}

#[tauri::command]
#[specta::specta]
pub async fn propose_course_plan(
    state: State<'_, AppState>,
    course_id: String,
    target_date: Option<String>,
) -> AppResult<PlanProposal> {
    let (p, open, info) = plan_prompt(&state, &course_id, target_date.as_deref())?;
    let (v, used) = ai::run(&state, plan_call(p, open.len(), &course_id)).await?;
    let raw: ai_study::RawPlan = serde_json::from_value(v)
        .map_err(|e| AppError::internal(format!("the AI returned an unexpected format: {e}")))?;
    let rationale = raw.rationale.clone();
    let (weeks, _) = ai_study::normalize_plan(raw, &open);
    let days = study::study_weekdays(&*state.db()?)?;
    let ids: Vec<Vec<String>> = weeks.iter().map(|w| w.1.clone()).collect();
    let dated: std::collections::HashMap<String, chrono::NaiveDate> =
        crate::services::progress::schedule::schedule_weeks(
            &ids,
            &days,
            chrono::Local::now().date_naive(),
        )
        .into_iter()
        .collect();
    Ok(PlanProposal {
        weeks: weeks
            .into_iter()
            .map(|(focus, ids)| PlanDraftWeek {
                focus,
                items: ids
                    .into_iter()
                    .map(|id| {
                        let (title, dur) = info.get(&id).cloned().unwrap_or_default();
                        PlanDraftItem {
                            date: dated
                                .get(&id)
                                .map(|d| d.format("%Y-%m-%d").to_string())
                                .unwrap_or_default(),
                            lesson_id: id,
                            title,
                            duration_sec: dur,
                        }
                    })
                    .collect(),
            })
            .collect(),
        rationale,
        cost_usd: used.cost_usd,
        model: used.model,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn apply_course_plan(
    state: State<'_, AppState>,
    course_id: String,
    items: Vec<PlanAssignment>,
) -> AppResult<CoursePlan> {
    let parsed: Vec<(String, chrono::NaiveDate)> = items
        .into_iter()
        .filter_map(|i| {
            chrono::NaiveDate::parse_from_str(&i.date, "%Y-%m-%d")
                .ok()
                .map(|d| (i.lesson_id, d))
        })
        .collect();
    let conn = state.db()?;
    study::apply_plan(&conn, &course_id, &parsed)?;
    study::get_course_plan(&conn, &course_id, chrono::Local::now().date_naive())
}
