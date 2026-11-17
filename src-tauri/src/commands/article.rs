//! On-demand article extraction and disk-backed caching.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::State;

use crate::db::{self, ArticleCache, Item};
use crate::fetch::article_fetch;

const SUCCESS_CACHE_TTL: i64 = 60 * 60 * 24 * 30;
const FAILURE_CACHE_TTL: i64 = 60 * 60 * 6;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArticleView {
    pub item_id: i64,
    pub original_url: Option<String>,
    pub final_url: Option<String>,
    pub title: Option<String>,
    pub content_html: Option<String>,
    pub rss_content_html: Option<String>,
    pub text_content: Option<String>,
    pub fetched_at: Option<i64>,
    pub cached: bool,
    pub error: Option<String>,
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn to_view(
    item_id: i64,
    cache: Option<ArticleCache>,
    original_url: Option<String>,
    cached: bool,
) -> ArticleView {
    if let Some(cache) = cache {
        ArticleView {
            item_id,
            original_url: Some(cache.original_url),
            final_url: cache.final_url,
            title: cache.title,
            content_html: cache.content_html,
            rss_content_html: None,
            text_content: cache.text_content,
            fetched_at: Some(cache.fetched_at),
            cached,
            error: cache.error,
        }
    } else {
        ArticleView {
            item_id,
            original_url,
            final_url: None,
            title: None,
            content_html: None,
            rss_content_html: None,
            text_content: None,
            fetched_at: None,
            cached: false,
            error: None,
        }
    }
}

fn item_for_article(conn: &rusqlite::Connection, item_id: i64) -> Result<Item, String> {
    db::get_item_by_id(conn, item_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("post {item_id} was not found"))
}

#[tauri::command]
pub async fn load_article(
    db_state: State<'_, std::sync::Mutex<rusqlite::Connection>>,
    item_id: i64,
    force_refresh: bool,
) -> Result<ArticleView, String> {
    let (item, cached) = {
        let conn = db_state
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        (
            item_for_article(&conn, item_id)?,
            db::get_article_cache(&conn, item_id).map_err(|error| error.to_string())?,
        )
    };
    if let Some(rss_content) = item.full_content.clone() {
        return Ok(ArticleView {
            item_id,
            original_url: item.link,
            final_url: None,
            title: item.title,
            content_html: None,
            rss_content_html: Some(rss_content),
            text_content: None,
            fetched_at: None,
            cached: false,
            error: None,
        });
    }
    let Some(original_url) = item.link.clone() else {
        return Ok(to_view(item_id, cached, None, true));
    };

    let now = now_unix();
    if !force_refresh
        && let Some(entry) = cached
            .as_ref()
            .filter(|entry| entry.original_url == original_url)
    {
        let ttl = if entry.error.is_some() {
            FAILURE_CACHE_TTL
        } else {
            SUCCESS_CACHE_TTL
        };
        if now.saturating_sub(entry.fetched_at) < ttl {
            return Ok(to_view(
                item_id,
                Some(entry.clone()),
                Some(original_url),
                true,
            ));
        }
    }

    let etag = cached.as_ref().and_then(|entry| entry.etag.clone());
    let last_modified = cached.as_ref().and_then(|entry| entry.last_modified.clone());
    match article_fetch::fetch_and_extract(&original_url, etag.as_deref(), last_modified.as_deref())
        .await
    {
        Ok(None) => {
            if let Some(mut previous) = cached {
                previous.fetched_at = now;
                previous.error = None;
                let conn = db_state
                    .lock()
                    .map_err(|_| "database lock poisoned".to_string())?;
                db::save_article_cache(&conn, &previous).map_err(|error| error.to_string())?;
                Ok(to_view(item_id, Some(previous), Some(original_url), true))
            } else {
                Err("The article server returned not-modified without a local copy.".to_string())
            }
        }
        Ok(Some(page)) => {
            let entry = ArticleCache {
                item_id,
                original_url: original_url.clone(),
                final_url: Some(page.final_url),
                title: Some(page.title),
                content_html: Some(page.content_html),
                text_content: Some(page.text_content),
                etag: page.etag,
                last_modified: page.last_modified,
                fetched_at: now,
                error: None,
            };
            let conn = db_state
                .lock()
                .map_err(|_| "database lock poisoned".to_string())?;
            db::save_article_cache(&conn, &entry).map_err(|error| error.to_string())?;
            Ok(to_view(item_id, Some(entry), Some(original_url), false))
        }
        Err(message) => {
            if let Some(mut previous) = cached
                .filter(|entry| entry.original_url == original_url && entry.content_html.is_some())
            {
                previous.error = Some(format!(
                    "Could not refresh the page. Showing the previously saved full text. {message}"
                ));
                return Ok(to_view(item_id, Some(previous), Some(original_url), true));
            }
            let entry = ArticleCache {
                item_id,
                original_url: original_url.clone(),
                final_url: None,
                title: None,
                content_html: None,
                text_content: None,
                etag: None,
                last_modified: None,
                fetched_at: now,
                error: Some(message),
            };
            let conn = db_state
                .lock()
                .map_err(|_| "database lock poisoned".to_string())?;
            db::save_article_cache(&conn, &entry).map_err(|error| error.to_string())?;
            Ok(to_view(item_id, Some(entry), Some(original_url), false))
        }
    }
}
