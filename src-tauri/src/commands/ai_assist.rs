//! AI study assist: explain a moment or page, answer a question, summarise a lesson, suggest
//! references. Every call shows a local estimate first; results are drafts until saved.

use tauri::State;

use crate::commands::ai::to_estimate;
use crate::db::models::*;
use crate::db::repo::{annotations, courses, lessons, notebooks};
use crate::error::{AppError, AppResult};
use crate::services::ai::{self, Call};
use crate::services::ai_assist as assist;
use crate::services::ai_context::{self, Context};
use crate::state::AppState;

/// Characters of material per request: a focused window is small; a whole lesson is capped.
const FOCUS_CHARS: usize = 40_000;
const LESSON_CHARS: usize = 120_000;

struct Prepared {
    ctx: Context,
    course_id: String,
    title: String,
    course_title: String,
}

/// Loads the lesson's material around the focus (or all of it) into an anchored context.
fn prepare(state: &AppState, focus: &AssistFocus, whole: bool) -> AppResult<Prepared> {
    let conn = state.db()?;
    let lesson = lessons::get_lesson(&conn, &focus.lesson_id)?;
    let course_id = lessons::course_id_of_lesson(&conn, &lesson.id)?;
    let course_title = courses::get_course(&conn, &course_id)?.title;
    let mut m = ai_context::load_lesson(&conn, &lesson.id, true)?;
    // A focused document that isn't among the lesson's linked PDFs (or beyond the first few).
    if let Some(mid) = &focus.material_id
        && !m.pdfs.iter().any(|(id, _, _)| id == mid)
    {
        let title = crate::db::repo::materials::get(&conn, mid)?.title;
        m.pdfs
            .push((mid.clone(), title, ai_context::material_pages(&conn, mid)?));
    }
    let m = if whole {
        m
    } else {
        assist::focus_material(m, focus)
    };
    if m.cues.is_empty()
        && m.notes.is_empty()
        && m.page_notes.is_empty()
        && m.pdfs.iter().all(|p| p.2.is_empty())
    {
        return Err(AppError::invalid(if focus.page.is_some() {
            "this page has no text to work from (it may be a scanned image)"
        } else {
            "there's no transcript or notes here to work from yet; fetch the transcript first"
        }));
    }
    let ctx = ai_context::build(&[m], if whole { LESSON_CHARS } else { FOCUS_CHARS });
    Ok(Prepared {
        ctx,
        course_id,
        title: lesson.title,
        course_title,
    })
}

fn assist_call<'a>(
    p: &Prepared,
    kind: AssistKind,
    focus: &'a AssistFocus,
    question: Option<&str>,
    system: &'a str,
) -> Call<'a> {
    let (feature, prompt, expected) = match kind {
        AssistKind::Explain => (
            "explain",
            assist::explain_prompt(&p.ctx, focus, question),
            assist::EXPECTED_ANSWER_OUTPUT,
        ),
        AssistKind::Answer => (
            "answer",
            assist::answer_prompt(&p.ctx, focus, question.unwrap_or("")),
            assist::EXPECTED_ANSWER_OUTPUT,
        ),
        AssistKind::Summary => (
            "summary",
            assist::summary_prompt(&p.ctx, &p.title),
            assist::EXPECTED_SUMMARY_OUTPUT,
        ),
    };
    Call {
        feature,
        system,
        prompt,
        schema: assist::answer_schema(),
        expected_output: expected,
        course_id: None,
        lesson_id: Some(&focus.lesson_id),
    }
}

fn system_for(kind: AssistKind) -> String {
    match kind {
        AssistKind::Explain => assist::explain_system(),
        AssistKind::Answer => assist::answer_system(),
        AssistKind::Summary => assist::summary_system(),
    }
}

fn check_question(kind: AssistKind, question: &Option<String>) -> AppResult<()> {
    if kind == AssistKind::Answer && question.as_deref().is_none_or(|q| q.trim().is_empty()) {
        return Err(AppError::invalid("write the question first"));
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn estimate_assist(
    state: State<'_, AppState>,
    kind: AssistKind,
    focus: AssistFocus,
    question: Option<String>,
) -> AppResult<AiEstimate> {
    check_question(kind, &question)?;
    let p = prepare(&state, &focus, kind == AssistKind::Summary)?;
    let system = system_for(kind);
    to_estimate(
        &state,
        &assist_call(&p, kind, &focus, question.as_deref(), &system),
        1,
    )
}

#[tauri::command]
#[specta::specta]
pub async fn run_assist(
    state: State<'_, AppState>,
    kind: AssistKind,
    focus: AssistFocus,
    question: Option<String>,
) -> AppResult<AssistAnswer> {
    check_question(kind, &question)?;
    let p = prepare(&state, &focus, kind == AssistKind::Summary)?;
    let system = system_for(kind);
    let mut call = assist_call(&p, kind, &focus, question.as_deref(), &system);
    call.course_id = Some(&p.course_id);
    let (v, used) = ai::run(&state, call).await?;
    let text = assist::parse_answer(v)
        .ok_or_else(|| AppError::internal("the AI returned an empty answer; try again"))?;
    let (markdown, sources) = assist::render_anchors(&text, &p.ctx);
    Ok(AssistAnswer {
        markdown,
        sources,
        cost_usd: used.cost_usd,
    })
}

/// Saves an AI answer onto an existing question (marked AI-generated and answered).
#[tauri::command]
#[specta::specta]
pub async fn save_ai_answer(
    state: State<'_, AppState>,
    annotation_id: String,
    markdown: String,
) -> AppResult<Annotation> {
    annotations::set_ai_answer(&*state.db()?, &annotation_id, &markdown)
}

/// Saves an explanation as a new question at the focused moment or page, already answered.
#[tauri::command]
#[specta::specta]
pub async fn save_explanation(
    state: State<'_, AppState>,
    focus: AssistFocus,
    title: String,
    markdown: String,
) -> AppResult<Annotation> {
    let conn = state.db()?;
    let q = annotations::create_with(
        &conn,
        &NewAnnotation {
            lesson_id: focus.lesson_id.clone(),
            kind: AnnotationType::Question,
            start_sec: focus.sec,
            end_sec: None,
            title: Some(title),
            body_md: None,
            status: None,
            priority: None,
            due_date: None,
            url: None,
            ref_type: None,
            answer_md: None,
            tags: None,
            material_id: focus.material_id.clone(),
            page: focus.page,
        },
        true,
    )?;
    annotations::set_ai_answer(&conn, &q.id, &markdown)
}

/// Appends an AI summary to the lesson notebook under a clearly marked heading.
#[tauri::command]
#[specta::specta]
pub async fn insert_lesson_summary(
    state: State<'_, AppState>,
    lesson_id: String,
    markdown: String,
) -> AppResult<Notebook> {
    let date = chrono::Local::now().format("%Y-%m-%d");
    let section = format!("## AI summary (generated {date})\n\n{}", markdown.trim());
    notebooks::append(&*state.db()?, NotebookScope::Lesson, &lesson_id, &section)
}

fn references_call<'a>(p: &Prepared, lesson_id: &'a str) -> Call<'a> {
    Call {
        feature: "references",
        system: assist::REFERENCES_SYSTEM,
        prompt: assist::references_prompt(&p.ctx, &p.title, &p.course_title),
        schema: assist::references_schema(),
        expected_output: assist::EXPECTED_REFERENCES_OUTPUT,
        course_id: None,
        lesson_id: Some(lesson_id),
    }
}

/// References need only the gist of the lesson, so they use a focused-size context.
fn prepare_refs(state: &AppState, lesson_id: &str) -> AppResult<Prepared> {
    let focus = AssistFocus {
        lesson_id: lesson_id.into(),
        sec: None,
        material_id: None,
        page: None,
    };
    let mut p = prepare(state, &focus, true)?;
    p.ctx.prompt = ai_context::truncate(&p.ctx.prompt, FOCUS_CHARS);
    Ok(p)
}

#[tauri::command]
#[specta::specta]
pub async fn estimate_references(
    state: State<'_, AppState>,
    lesson_id: String,
) -> AppResult<AiEstimate> {
    let p = prepare_refs(&state, &lesson_id)?;
    to_estimate(&state, &references_call(&p, &lesson_id), 1)
}

#[tauri::command]
#[specta::specta]
pub async fn suggest_references(
    state: State<'_, AppState>,
    lesson_id: String,
) -> AppResult<ReferenceSuggestions> {
    let p = prepare_refs(&state, &lesson_id)?;
    let mut call = references_call(&p, &lesson_id);
    call.course_id = Some(&p.course_id);
    let (v, used) = ai::run(&state, call).await?;
    let items = assist::parse_references(v);
    if items.is_empty() {
        return Err(AppError::internal(
            "the AI didn't suggest any references it was confident about",
        ));
    }
    Ok(ReferenceSuggestions {
        items,
        cost_usd: used.cost_usd,
    })
}

/// Saves chosen suggestions as reference annotations (AI-generated, no URL).
#[tauri::command]
#[specta::specta]
pub async fn save_reference_drafts(
    state: State<'_, AppState>,
    lesson_id: String,
    drafts: Vec<ReferenceDraft>,
) -> AppResult<i32> {
    let conn = state.db()?;
    let mut n = 0;
    for d in drafts {
        let body = [d.author.clone().unwrap_or_default(), d.why.clone()]
            .into_iter()
            .filter(|s| !s.trim().is_empty())
            .collect::<Vec<_>>()
            .join(" — ");
        let r = annotations::create_with(
            &conn,
            &NewAnnotation {
                lesson_id: lesson_id.clone(),
                kind: AnnotationType::Reference,
                start_sec: None,
                end_sec: None,
                title: Some(d.title),
                body_md: Some(body),
                status: None,
                priority: None,
                due_date: None,
                url: None,
                ref_type: Some(d.ref_type),
                answer_md: None,
                tags: None,
                material_id: None,
                page: None,
            },
            true,
        );
        if r.is_ok() {
            n += 1;
        }
    }
    Ok(n)
}
