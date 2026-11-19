//! Shared, live admission budgets. Reducing a budget never kills running work.
use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use serde::Serialize;
use tokio::sync::{Notify, watch};

use crate::db::resources::{self, ResourceSettings, ResourceSnapshot};

/// The global floor affects normal auto polls, not the existing error retry
/// schedule. New subscriptions remain eligible for their first fetch.
pub fn effective_due_at(meta: &crate::db::FeedMeta, minimum_secs: u32) -> i64 {
    if meta.last_checked_at == 0 || meta.consecutive_failures > 0 {
        meta.next_fetch_at
    } else {
        meta.next_fetch_at
            .max(meta.last_checked_at.saturating_add(i64::from(minimum_secs)))
    }
}

#[derive(Debug, Clone, Copy)]
pub enum WorkKind {
    Feed,
    Article,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceStatus {
    pub snapshot: ResourceSnapshot,
    pub recovery_error: Option<String>,
    pub running_work: usize,
}

struct PolicyState {
    snapshot: ResourceSnapshot,
    recovery_error: Option<String>,
    feeds: usize,
    articles: usize,
    native_hidden: bool,
    document_hidden: bool,
}

struct Shared {
    state: Mutex<PolicyState>,
    available: Notify,
    changed: watch::Sender<u64>,
}

#[derive(Clone)]
pub struct ResourcePolicy(Arc<Shared>);

impl Default for ResourcePolicy {
    fn default() -> Self {
        Self::new(
            ResourceSnapshot {
                revision: 0,
                settings: ResourceSettings::default(),
            },
            None,
        )
    }
}

impl ResourcePolicy {
    #[cfg(test)]
    pub(crate) fn with_prefetch_for_tests() -> Self {
        let policy = Self::default();
        let mut settings = policy.settings();
        settings.mode = resources::ResourceMode::Custom;
        settings.prefetch_full_text = true;
        policy.apply(ResourceSnapshot {
            revision: 1,
            settings,
        });
        policy
    }

    fn new(snapshot: ResourceSnapshot, recovery_error: Option<String>) -> Self {
        let (changed, _) = watch::channel(0);
        Self(Arc::new(Shared {
            state: Mutex::new(PolicyState {
                snapshot,
                recovery_error,
                feeds: 0,
                articles: 0,
                native_hidden: false,
                document_hidden: false,
            }),
            available: Notify::new(),
            changed,
        }))
    }

    /// Keep reading available even if persisted policy JSON needs explicit repair.
    pub fn load(conn: &Connection) -> Result<Self, String> {
        match resources::get(conn) {
            Ok(snapshot) => Ok(Self::new(snapshot, None)),
            Err(error) => {
                let revision = conn
                    .query_row(
                        "SELECT revision FROM resource_settings WHERE id=1",
                        [],
                        |r| r.get(0),
                    )
                    .map_err(|e| e.to_string())?;
                Ok(Self::new(
                    ResourceSnapshot {
                        revision,
                        settings: ResourceSettings::default(),
                    },
                    Some(error),
                ))
            }
        }
    }

    pub fn status(&self) -> ResourceStatus {
        let state = self.0.state.lock().expect("resource policy poisoned");
        ResourceStatus {
            snapshot: state.snapshot.clone(),
            recovery_error: state.recovery_error.clone(),
            running_work: state.feeds + state.articles,
        }
    }

    pub fn settings(&self) -> ResourceSettings {
        self.0
            .state
            .lock()
            .expect("resource policy poisoned")
            .snapshot
            .settings
            .clone()
    }

    pub fn apply(&self, snapshot: ResourceSnapshot) {
        {
            let mut state = self.0.state.lock().expect("resource policy poisoned");
            // Late notifications must never roll the runtime back.
            if snapshot.revision <= state.snapshot.revision {
                return;
            }
            state.snapshot = snapshot;
            state.recovery_error = None;
        }
        self.signal_change();
    }

    fn signal_change(&self) {
        self.0
            .changed
            .send_modify(|generation| *generation = generation.wrapping_add(1));
        self.0.available.notify_waiters();
    }

    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.0.changed.subscribe()
    }

    pub fn set_document_hidden(&self, hidden: bool) {
        self.set_hidden(hidden, false);
    }
    pub fn set_native_hidden(&self, hidden: bool) {
        self.set_hidden(hidden, true);
    }

    fn set_hidden(&self, hidden: bool, native: bool) {
        {
            let mut state = self.0.state.lock().expect("resource policy poisoned");
            let current = if native {
                &mut state.native_hidden
            } else {
                &mut state.document_hidden
            };
            if *current == hidden {
                return;
            }
            *current = hidden;
        }
        self.signal_change();
    }

    fn paused(state: &PolicyState) -> bool {
        state.snapshot.settings.pause_optional_when_hidden
            && (state.native_hidden || state.document_hidden)
    }

    pub fn optional_allowed(&self) -> bool {
        !Self::paused(&self.0.state.lock().expect("resource policy poisoned"))
    }

    /// Optional article work is skipped (not parked holding a feed slot) if
    /// disabled or hidden. Required reading always remains eligible.
    pub async fn acquire(&self, kind: WorkKind, optional: bool) -> Option<WorkPermit> {
        loop {
            let notified = self.0.available.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let mut state = self.0.state.lock().expect("resource policy poisoned");
                if optional && (Self::paused(&state) || !state.snapshot.settings.prefetch_full_text)
                {
                    return None;
                }
                let (active, limit) = match kind {
                    WorkKind::Feed => (state.feeds, state.snapshot.settings.feed_concurrency),
                    WorkKind::Article => {
                        (state.articles, state.snapshot.settings.article_concurrency)
                    }
                };
                if active < limit as usize {
                    match kind {
                        WorkKind::Feed => state.feeds += 1,
                        WorkKind::Article => state.articles += 1,
                    }
                    return Some(WorkPermit {
                        policy: self.clone(),
                        kind,
                    });
                }
            }
            notified.await;
        }
    }
}

pub struct WorkPermit {
    policy: ResourcePolicy,
    kind: WorkKind,
}

impl Drop for WorkPermit {
    fn drop(&mut self) {
        {
            let mut state = self
                .policy
                .0
                .state
                .lock()
                .expect("resource policy poisoned");
            match self.kind {
                WorkKind::Feed => state.feeds -= 1,
                WorkKind::Article => state.articles -= 1,
            }
        }
        self.policy.0.available.notify_waiters();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::resources::ResourceMode;
    use std::time::Duration;

    #[tokio::test]
    async fn shrink_preserves_running_work_and_blocks_new_admission_until_below_limit() {
        let policy = ResourcePolicy::default();
        let first = policy.acquire(WorkKind::Feed, false).await.unwrap();
        let second = policy.acquire(WorkKind::Feed, false).await.unwrap();
        policy.apply(ResourceSnapshot {
            revision: 1,
            settings: ResourceSettings::preset(ResourceMode::Light).unwrap(),
        });
        let pending = policy.acquire(WorkKind::Feed, false);
        tokio::pin!(pending);
        assert!(
            tokio::time::timeout(Duration::from_millis(10), &mut pending)
                .await
                .is_err()
        );
        drop(first);
        assert!(
            tokio::time::timeout(Duration::from_millis(10), &mut pending)
                .await
                .is_err()
        );
        drop(second);
        let _next = tokio::time::timeout(Duration::from_secs(1), &mut pending)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(policy.status().running_work, 1);
    }

    #[tokio::test]
    async fn disabling_prefetch_wakes_waiters_but_does_not_block_required_reading() {
        let policy = ResourcePolicy::with_prefetch_for_tests();
        let first = policy.acquire(WorkKind::Article, false).await.unwrap();
        let second = policy.acquire(WorkKind::Article, false).await.unwrap();
        let pending = policy.acquire(WorkKind::Article, true);
        tokio::pin!(pending);
        assert!(
            tokio::time::timeout(Duration::from_millis(10), &mut pending)
                .await
                .is_err()
        );
        policy.apply(ResourceSnapshot {
            revision: 2,
            settings: ResourceSettings::preset(ResourceMode::Light).unwrap(),
        });
        assert!(
            tokio::time::timeout(Duration::from_secs(1), &mut pending)
                .await
                .unwrap()
                .is_none()
        );
        drop(first);
        drop(second);
        policy.set_document_hidden(true);
        assert!(!policy.optional_allowed());
        let _required = policy.acquire(WorkKind::Article, false).await.unwrap();
        assert!(policy.acquire(WorkKind::Article, true).await.is_none());
        policy.set_document_hidden(false);
        assert!(policy.optional_allowed());
    }

    #[test]
    fn damaged_saved_settings_are_visible_and_only_a_newer_valid_save_clears_recovery() {
        let conn = crate::db::init_in_memory().unwrap();
        conn.execute("UPDATE resource_settings SET settings_json='damaged'", [])
            .unwrap();
        let policy = ResourcePolicy::load(&conn).unwrap();
        assert!(policy.status().recovery_error.is_some());
        let snapshot = resources::save(&conn, 0, ResourceSettings::default()).unwrap();
        policy.apply(snapshot.clone());
        assert!(policy.status().recovery_error.is_none());
        policy.apply(ResourceSnapshot {
            revision: 0,
            settings: ResourceSettings::preset(ResourceMode::Light).unwrap(),
        });
        assert_eq!(policy.status().snapshot, snapshot);
    }
}
