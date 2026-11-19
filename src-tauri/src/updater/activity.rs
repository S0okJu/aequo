//! A shared gate closes the race between starting work and installing/restarting.
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};

#[derive(Default)]
struct State {
    work: usize,
    exclusive: bool,
}

#[derive(Clone, Default)]
pub struct Activity(Arc<Mutex<State>>);
pub struct Work(Activity);
pub struct Exclusive(Activity);

impl Activity {
    pub fn begin(&self) -> Result<Work, String> {
        let mut state = self.0.lock().unwrap();
        if state.exclusive {
            return Err("update_busy".into());
        }
        state.work += 1;
        Ok(Work(self.clone()))
    }
    pub fn is_busy(&self) -> bool {
        self.0.lock().unwrap().work > 0
    }
    pub fn exclusive(&self) -> Option<Exclusive> {
        let mut state = self.0.lock().unwrap();
        if state.work > 0 || state.exclusive {
            return None;
        }
        state.exclusive = true;
        Some(Exclusive(self.clone()))
    }
}
impl Drop for Work {
    fn drop(&mut self) {
        self.0.0.lock().unwrap().work -= 1;
    }
}
impl Drop for Exclusive {
    fn drop(&mut self) {
        self.0.0.lock().unwrap().exclusive = false;
    }
}
pub fn begin(app: &AppHandle) -> Result<Option<Work>, String> {
    app.try_state::<Activity>()
        .map(|gate| gate.begin())
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn install_and_user_work_cannot_overlap() {
        let activity = Activity::default();
        let work = activity.begin().unwrap();
        assert!(activity.exclusive().is_none());
        drop(work);
        let install = activity.exclusive().unwrap();
        assert!(activity.begin().is_err());
        drop(install);
        assert!(activity.begin().is_ok());
    }
}
