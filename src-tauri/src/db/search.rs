//! Full-text search over items (docs/UI_SIMPLIFICATION_PLAN.md §3.14).
//!
//! `items_fts` is a contentless FTS5 table with the `trigram` tokenizer:
//! trigram matching is substring matching, so Korean words with particles
//! ("블로그에서") are found by their stem ("블로그"), which word tokenizers
//! miss. It indexes the title, summary and RSS body as plain text (HTML
//! stripped by the `strip_html` SQL function registered on every
//! connection) - not extracted article text, which only exists for posts
//! that were opened and is cleared by retention.
//!
//! Triggers keep the index in step with `items`, so the retention policy's
//! bulk UPDATE/DELETE statements need no extra bookkeeping. Queries under
//! three characters can't use a trigram index and fall back to a LIKE over
//! title and summary.

use rusqlite::functions::FunctionFlags;
use rusqlite::{Connection, Result};

/// Search terms shorter than this (in characters) use the LIKE fallback.
pub const MIN_INDEXED_QUERY_CHARS: usize = 3;

const SCHEMA: &str = "
CREATE VIRTUAL TABLE IF NOT EXISTS items_fts USING fts5(
    title, summary, body,
    content='', contentless_delete=1, tokenize='trigram'
);

CREATE TRIGGER IF NOT EXISTS items_fts_insert AFTER INSERT ON items BEGIN
    INSERT INTO items_fts(rowid, title, summary, body)
    VALUES (new.id, new.title, strip_html(new.summary), strip_html(new.full_content));
END;

CREATE TRIGGER IF NOT EXISTS items_fts_delete AFTER DELETE ON items BEGIN
    DELETE FROM items_fts WHERE rowid = old.id;
END;

-- Polling rewrites full_content with COALESCE on every item, so only
-- re-index when an indexed column actually changed.
CREATE TRIGGER IF NOT EXISTS items_fts_update AFTER UPDATE OF title, summary, full_content ON items
WHEN old.title IS NOT new.title
  OR old.summary IS NOT new.summary
  OR old.full_content IS NOT new.full_content
BEGIN
    DELETE FROM items_fts WHERE rowid = old.id;
    INSERT INTO items_fts(rowid, title, summary, body)
    VALUES (new.id, new.title, strip_html(new.summary), strip_html(new.full_content));
END;
";

/// Registers `strip_html(text)`; must run on every connection before any
/// write to `items` (the index triggers call it).
pub fn register_functions(conn: &Connection) -> Result<()> {
    conn.create_scalar_function(
        "strip_html",
        1,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let html: Option<String> = ctx.get(0)?;
            Ok(html.map(|html| strip_html(&html)))
        },
    )
}

/// Creates the index and triggers; on first creation, indexes existing items.
pub fn apply(conn: &Connection) -> Result<()> {
    let existed: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='items_fts')",
        [],
        |row| row.get(0),
    )?;
    conn.execute_batch(SCHEMA)?;
    if !existed {
        conn.execute(
            "INSERT INTO items_fts(rowid, title, summary, body)
             SELECT id, title, strip_html(summary), strip_html(full_content) FROM items",
            [],
        )?;
    }
    Ok(())
}

/// A WHERE condition (on `items`) for `query`, with one value per `?`.
/// Indexed queries are passed as one quoted FTS5 phrase, so characters like
/// `-`, `*` or `AND` in what the user typed are matched literally.
pub fn query_condition(query: &str) -> (&'static str, Vec<String>) {
    if query.chars().count() >= MIN_INDEXED_QUERY_CHARS {
        (
            "id IN (SELECT rowid FROM items_fts WHERE items_fts MATCH ?)",
            vec![format!("\"{}\"", query.replace('"', "\"\""))],
        )
    } else {
        let escaped = query
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let pattern = format!("%{escaped}%");
        (
            "(title LIKE ? ESCAPE '\\' OR summary LIKE ? ESCAPE '\\')",
            vec![pattern.clone(), pattern],
        )
    }
}

/// Tags removed, common entities decoded, whitespace collapsed.
pub fn strip_html(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => {
                in_tag = true;
                text.push(' ');
            }
            '>' if in_tag => in_tag = false,
            _ if !in_tag => text.push(ch),
            _ => {}
        }
    }
    let decoded = text
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#039;", "'")
        .replace("&amp;", "&");
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_tags_and_decodes_entities() {
        assert_eq!(
            strip_html("<p>Tom &amp; Jerry&nbsp;<b>run</b></p>\n<p>far</p>"),
            "Tom & Jerry run far"
        );
    }

    #[test]
    fn short_queries_use_an_escaped_like() {
        let (sql, values) = query_condition("5%");
        assert!(sql.contains("LIKE"));
        assert_eq!(values, ["%5\\%%", "%5\\%%"]);
        let (sql, values) = query_condition("say \"hi\"");
        assert!(sql.contains("MATCH"));
        assert_eq!(values, ["\"say \"\"hi\"\"\""]);
    }
}
