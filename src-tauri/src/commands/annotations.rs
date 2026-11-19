//! Highlight/memo annotation commands (see docs/READER_ANNOTATIONS_PLAN.md
//! §3, §5). Anchoring (plain-text offsets, content hashing, self-healing
//! re-anchoring) all lives in the frontend (`src/lib/annotation.ts`) - this
//! module only validates and persists the rows it's given.

use std::sync::Mutex;

use rusqlite::Connection;
use tauri::{AppHandle, State};

use crate::db;

const VALID_COLORS: [&str; 5] = ["yellow", "green", "blue", "pink", "purple"];
const MAX_NOTE_LEN: usize = 10_000;
const MAX_QUOTED_TEXT_LEN: usize = 20_000;

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn validate_quoted_text(quoted_text: &str) -> Result<(), String> {
    if quoted_text.is_empty() {
        return Err("quoted text cannot be empty".to_string());
    }
    if quoted_text.len() > MAX_QUOTED_TEXT_LEN {
        return Err("quoted text is too long".to_string());
    }
    Ok(())
}

fn validate_offsets(start_offset: i64, end_offset: i64) -> Result<(), String> {
    if start_offset < 0 || end_offset <= start_offset {
        return Err("invalid annotation range".to_string());
    }
    Ok(())
}

/// Checks the `kind`-dependent shape of a highlight/memo: a highlight
/// requires an allowlisted `color` and no `note`; a memo requires a
/// non-empty, length-limited `note` and no `color`. Kept separate from
/// `validate_offsets`/`validate_quoted_text` so each rule can be tested
/// in isolation.
fn validate_kind_fields(kind: &str, color: Option<&str>, note: Option<&str>) -> Result<(), String> {
    match kind {
        "highlight" => {
            let color = color.ok_or("a highlight requires a color")?;
            if !VALID_COLORS.contains(&color) {
                return Err(format!("unsupported highlight color: {color}"));
            }
            if note.is_some() {
                return Err("a highlight cannot have a note".to_string());
            }
            Ok(())
        }
        "memo" => {
            if color.is_some() {
                return Err("a memo cannot have a color".to_string());
            }
            let note = note.ok_or("a memo requires a note")?;
            if note.trim().is_empty() {
                return Err("memo note cannot be empty".to_string());
            }
            if note.len() > MAX_NOTE_LEN {
                return Err("memo note is too long".to_string());
            }
            Ok(())
        }
        _ => Err(format!("unsupported annotation kind: {kind}")),
    }
}

/// An overlap is only acceptable when exactly one other annotation
/// occupies the identical range and it is the opposite kind (a
/// highlight and a memo sharing one quoted span) - any other overlap
/// is rejected (see docs/READER_ANNOTATIONS_PLAN.md open questions,
/// resolved: disallow general overlap).
fn check_overlap(
    overlapping: &[db::Annotation],
    kind: &str,
    start_offset: i64,
    end_offset: i64,
) -> Result<(), String> {
    let allowed = overlapping.len() == 1
        && overlapping[0].kind != kind
        && overlapping[0].start_offset == start_offset
        && overlapping[0].end_offset == end_offset;
    if overlapping.is_empty() || allowed {
        Ok(())
    } else {
        Err("this range overlaps an existing annotation".to_string())
    }
}

#[tauri::command]
pub fn list_item_annotations(
    db_state: State<'_, Mutex<Connection>>,
    item_id: i64,
) -> Result<Vec<db::Annotation>, String> {
    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    db::list_annotations(&conn, item_id).map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn create_annotation(
    app: AppHandle,
    db_state: State<'_, Mutex<Connection>>,
    item_id: i64,
    kind: String,
    content_hash: String,
    start_offset: i64,
    end_offset: i64,
    quoted_text: String,
    color: Option<String>,
    note: Option<String>,
) -> Result<db::Annotation, String> {
    let _activity = crate::updater::activity::begin(&app)?;
    validate_offsets(start_offset, end_offset)?;
    validate_quoted_text(&quoted_text)?;
    validate_kind_fields(&kind, color.as_deref(), note.as_deref())?;

    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;

    let overlapping =
        db::annotations_overlapping(&conn, item_id, &content_hash, start_offset, end_offset)
            .map_err(|e| e.to_string())?;
    check_overlap(&overlapping, &kind, start_offset, end_offset)?;

    let now = now_unix();
    let id = db::create_annotation(
        &conn,
        item_id,
        &kind,
        &content_hash,
        start_offset,
        end_offset,
        &quoted_text,
        color.as_deref(),
        note.as_deref(),
        now,
    )
    .map_err(|e| e.to_string())?;

    db::get_annotation(&conn, id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "annotation vanished immediately after creation".to_string())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn update_annotation(
    app: AppHandle,
    db_state: State<'_, Mutex<Connection>>,
    id: i64,
    kind: String,
    content_hash: String,
    start_offset: i64,
    end_offset: i64,
    quoted_text: String,
    color: Option<String>,
    note: Option<String>,
) -> Result<db::Annotation, String> {
    let _activity = crate::updater::activity::begin(&app)?;
    validate_offsets(start_offset, end_offset)?;
    validate_quoted_text(&quoted_text)?;
    validate_kind_fields(&kind, color.as_deref(), note.as_deref())?;

    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;

    let updated = db::update_annotation(
        &conn,
        id,
        &content_hash,
        start_offset,
        end_offset,
        &quoted_text,
        color.as_deref(),
        note.as_deref(),
        now_unix(),
    )
    .map_err(|e| e.to_string())?;
    if !updated {
        return Err(format!("annotation {id} not found"));
    }

    db::get_annotation(&conn, id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("annotation {id} not found"))
}

#[tauri::command]
pub fn delete_annotation(
    app: AppHandle,
    db_state: State<'_, Mutex<Connection>>,
    id: i64,
) -> Result<(), String> {
    let _activity = crate::updater::activity::begin(&app)?;
    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    if db::delete_annotation(&conn, id).map_err(|e| e.to_string())? {
        Ok(())
    } else {
        Err(format!("annotation {id} not found"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlight_requires_allowlisted_color_and_no_note() {
        assert!(validate_kind_fields("highlight", Some("yellow"), None).is_ok());
        assert!(validate_kind_fields("highlight", None, None).is_err());
        assert!(validate_kind_fields("highlight", Some("chartreuse"), None).is_err());
        assert!(validate_kind_fields("highlight", Some("yellow"), Some("note")).is_err());
    }

    #[test]
    fn memo_requires_nonempty_note_and_no_color() {
        assert!(validate_kind_fields("memo", None, Some("hello")).is_ok());
        assert!(validate_kind_fields("memo", None, None).is_err());
        assert!(validate_kind_fields("memo", None, Some("   ")).is_err());
        assert!(validate_kind_fields("memo", Some("yellow"), Some("hello")).is_err());
    }

    #[test]
    fn memo_note_length_is_capped() {
        let too_long = "a".repeat(MAX_NOTE_LEN + 1);
        assert!(validate_kind_fields("memo", None, Some(&too_long)).is_err());
        let exactly_max = "a".repeat(MAX_NOTE_LEN);
        assert!(validate_kind_fields("memo", None, Some(&exactly_max)).is_ok());
    }

    #[test]
    fn unsupported_kind_is_rejected() {
        assert!(validate_kind_fields("bogus", Some("yellow"), None).is_err());
    }

    #[test]
    fn quoted_text_length_is_bounded() {
        assert!(validate_quoted_text("").is_err());
        assert!(validate_quoted_text(&"a".repeat(MAX_QUOTED_TEXT_LEN + 1)).is_err());
        assert!(validate_quoted_text(&"a".repeat(MAX_QUOTED_TEXT_LEN)).is_ok());
    }

    #[test]
    fn offsets_must_be_a_nonempty_forward_range() {
        assert!(validate_offsets(0, 5).is_ok());
        assert!(validate_offsets(5, 5).is_err());
        assert!(validate_offsets(5, 0).is_err());
        assert!(validate_offsets(-1, 5).is_err());
    }

    fn sample_annotation(kind: &str, start_offset: i64, end_offset: i64) -> db::Annotation {
        db::Annotation {
            id: 1,
            item_id: 1,
            kind: kind.to_string(),
            content_hash: "hash-1".to_string(),
            start_offset,
            end_offset,
            quoted_text: "hello".to_string(),
            color: None,
            note: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn no_overlap_is_always_allowed() {
        assert!(check_overlap(&[], "highlight", 0, 5).is_ok());
    }

    #[test]
    fn identical_range_different_kind_is_allowed() {
        let existing = [sample_annotation("memo", 0, 5)];
        assert!(check_overlap(&existing, "highlight", 0, 5).is_ok());
    }

    #[test]
    fn identical_range_same_kind_is_rejected() {
        let existing = [sample_annotation("highlight", 0, 5)];
        assert!(check_overlap(&existing, "highlight", 0, 5).is_err());
    }

    #[test]
    fn partial_overlap_is_rejected_even_for_the_opposite_kind() {
        let existing = [sample_annotation("memo", 0, 5)];
        assert!(check_overlap(&existing, "highlight", 2, 8).is_err());
    }

    #[test]
    fn multiple_overlaps_are_rejected() {
        let existing = [
            sample_annotation("memo", 0, 5),
            sample_annotation("highlight", 0, 5),
        ];
        assert!(check_overlap(&existing, "memo", 0, 5).is_err());
    }
}
