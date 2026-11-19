//! The profile being edited, its saved versions, autosave and error texts
//! (Swift `PluginViewModel` profile part and `AudioCurveAudioUnit` autosave).
//! Holds no platform code; the caller passes time in as `Instant`s.

use std::time::{Duration, Instant};

use audiocurve_core::editing::PointSelection;
use audiocurve_core::hearing_test::HearingTestRecord;
use audiocurve_core::profile::{Ear, HearingProfile};
use audiocurve_core::store::types::{SavedProfile, WorkingCopy};
use audiocurve_core::store::{ProfileStore, StoreError};
use uuid::Uuid;

use crate::i18n::{tr, trf};
use crate::model::editing_input::DragState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane {
    Audiogram,
    Gain,
    Spectrum,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrimSelection {
    pub ear: Ear,
    pub band: usize,
}

const AUTOSAVE_DELAY: Duration = Duration::from_millis(500);
const ERROR_DURATION: Duration = Duration::from_secs(3);

pub struct Editor {
    store: ProfileStore,
    pub profile: Option<HearingProfile>,
    /// The saved version the profile belongs to.
    pub source_id: Option<Uuid>,
    pub saved: Vec<SavedProfile>,
    pub selection: Option<PointSelection>,
    pub trim_selection: Option<TrimSelection>,
    pub active_ear: Ear,
    pub pane: Pane,
    /// Digits typed for the selected point, shown in its label until Enter or Esc.
    pub typed_buffer: String,
    pub error: Option<String>,
    error_until: Option<Instant>,
    autosave_at: Option<Instant>,
    /// The pointer drag in progress (see `editing_input`).
    pub(crate) drag: Option<DragState>,
}

impl Editor {
    pub fn new(store: ProfileStore) -> Editor {
        let working = store.load_working_copy();
        let saved = store.list();
        Editor {
            profile: working.as_ref().map(|w| w.profile.clone()),
            source_id: working.and_then(|w| w.source_id),
            saved,
            store,
            selection: None,
            trim_selection: None,
            active_ear: Ear::Left,
            pane: Pane::Audiogram,
            typed_buffer: String::new(),
            error: None,
            error_until: None,
            autosave_at: None,
            drag: None,
        }
    }

    pub fn current_name(&self) -> Option<&str> {
        let id = self.source_id?;
        self.saved
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.name.as_str())
    }

    pub fn is_dirty(&self) -> bool {
        let Some(p) = &self.profile else { return false };
        let Some(id) = self.source_id else {
            return true;
        };
        match self.saved.iter().find(|s| s.id == id) {
            Some(s) => s.profile != *p,
            None => true,
        }
    }

    pub fn refresh_saved(&mut self) {
        self.saved = self.store.list();
    }

    pub fn load(&mut self, id: Uuid) -> bool {
        self.refresh_saved();
        self.clear_selection();
        let Some(saved) = self.store.saved(id) else {
            self.show_error(tr("Could not load the profile"));
            return false;
        };
        self.set_source(Some(id));
        self.profile = Some(saved.profile);
        self.schedule_autosave(Instant::now());
        true
    }

    /// Overwrites the current version. Returns false when there is none, so the UI can ask for a name.
    pub fn save(&mut self) -> bool {
        self.refresh_saved();
        let Some(id) = self
            .source_id
            .filter(|id| self.saved.iter().any(|s| s.id == *id))
        else {
            return false;
        };
        if let Some(p) = self.profile.clone() {
            let r = self.store.overwrite(id, &p);
            self.finish(r.map(|_| ()));
        }
        true
    }

    pub fn save_as(&mut self, name: &str) {
        let Some(p) = self.profile.clone() else {
            return;
        };
        let r = self
            .store
            .save_new(name, &p)
            .map(|s| self.set_source(Some(s.id)));
        self.finish(r);
    }

    pub fn rename(&mut self, id: Uuid, name: &str) {
        let r = self.store.rename(id, name);
        self.finish(r);
    }

    pub fn duplicate(&mut self, id: Uuid) {
        let r = self
            .store
            .duplicate(id, |base, n| {
                if n == 1 {
                    trf("%@ copy", &[base])
                } else {
                    trf("%@ copy %lld", &[base, &n.to_string()])
                }
            })
            .map(|_| ());
        self.finish(r);
    }

    pub fn delete(&mut self, id: Uuid) {
        let r = self.store.delete(id).map(|_| {
            if self.source_id == Some(id) {
                self.set_source(None);
            }
        });
        self.finish(r);
    }

    pub fn add_flat_profile(&mut self) {
        self.clear_selection();
        self.set_profile(HearingProfile::flat(), Instant::now());
    }

    /// Makes `p` the current profile and schedules an autosave 0.5 s from `now`.
    pub fn set_profile(&mut self, p: HearingProfile, now: Instant) {
        if self.profile.as_ref() == Some(&p) {
            return;
        }
        if p.validate().is_err() {
            self.show_error(tr("Invalid profile"));
            return;
        }
        self.profile = Some(p);
        self.schedule_autosave(now);
    }

    /// Writes a due autosave and clears an expired error.
    pub fn tick(&mut self, now: Instant) {
        if self.autosave_at.is_some_and(|t| now >= t) {
            self.flush_autosave();
        }
        if self.error_until.is_some_and(|t| now >= t) {
            self.error = None;
            self.error_until = None;
        }
    }

    /// Writes a pending autosave at once (for quitting or going to the background).
    pub fn flush_autosave(&mut self) {
        if self.autosave_at.take().is_none() {
            return;
        }
        if let Some(p) = &self.profile {
            self.store.save_working_copy(&WorkingCopy {
                profile: p.clone(),
                source_id: self.source_id,
            });
        }
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        match (self.autosave_at, self.error_until) {
            (Some(a), Some(e)) => Some(a.min(e)),
            (a, e) => a.or(e),
        }
    }

    /// Saves a hearing-test result under a unique name (idempotent on `id`) and refreshes the
    /// saved list. It is not loaded: as in Swift, it becomes the profile only by being assigned
    /// to the current output (`AppModel::assign`), which saves unsaved edits first.
    pub fn save_measured(
        &mut self,
        id: Uuid,
        name: &str,
        p: HearingProfile,
        m: HearingTestRecord,
    ) -> Result<SavedProfile, StoreError> {
        let saved = match self.store.saved(id) {
            Some(s) => s,
            None => self.store.save_new_with(id, name, &p, Some(m), true)?,
        };
        self.refresh_saved();
        Ok(saved)
    }

    fn clear_selection(&mut self) {
        self.selection = None;
        self.trim_selection = None;
        self.typed_buffer.clear();
    }

    fn set_source(&mut self, id: Option<Uuid>) {
        if self.source_id != id {
            self.source_id = id;
            self.schedule_autosave(Instant::now());
        }
    }

    fn schedule_autosave(&mut self, now: Instant) {
        self.autosave_at = self.profile.is_some().then_some(now + AUTOSAVE_DELAY);
    }

    pub(crate) fn show_error(&mut self, text: String) {
        self.error = Some(text);
        self.error_until = Some(Instant::now() + ERROR_DURATION);
    }

    /// Reports a store error as text, then re-reads the saved versions.
    fn finish(&mut self, r: Result<(), StoreError>) {
        if let Err(e) = r {
            self.show_error(match e {
                StoreError::EmptyName => tr("Enter a name"),
                StoreError::NameTaken => tr("A version with that name already exists"),
                StoreError::NotFound | StoreError::Io(_) => tr("Could not save the profile"),
            });
        }
        self.refresh_saved();
    }
}
