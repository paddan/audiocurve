mod support;

use std::collections::HashSet;

use audiocurve_core::outputs::{
    LevelSettings, OutputAssignment, OutputAssignments, OutputDevice, OutputSwitchAction as A, output_switch_actions,
};
use audiocurve_core::profile::HearingProfile;
use audiocurve_core::store::types::WorkingCopy;
use support::example_profile;
use uuid::Uuid;

struct Ids {
    a: Uuid,
    b: Uuid,
}

fn ids() -> Ids {
    Ids { a: Uuid::new_v4(), b: Uuid::new_v4() }
}

const LV: LevelSettings = LevelSettings { strength: 0.8, input_gain_db: 1.0, output_gain_db: -2.0, tone: 0.1, mpo_dbfs: -6.0 };

fn edited() -> HearingProfile {
    example_profile()
}

fn assignment(profile: Option<Uuid>, levels: Option<LevelSettings>) -> OutputAssignment {
    OutputAssignment { device: OutputDevice { uid: "u1".into(), name: "Dev".into() }, profile_id: profile, levels }
}

fn set(v: &[Uuid]) -> HashSet<Uuid> {
    v.iter().copied().collect()
}

fn wc(profile: HearingProfile, source: Option<Uuid>) -> WorkingCopy {
    WorkingCopy { profile, source_id: source }
}

#[test]
fn no_assignment_does_nothing() {
    let i = ids();
    let w = wc(edited(), Some(i.a));
    assert_eq!(output_switch_actions(None, Some(&w), Some(&HearingProfile::flat()), &set(&[i.a, i.b])), vec![]);
}

#[test]
fn no_working_copy_just_loads() {
    let i = ids();
    assert_eq!(output_switch_actions(Some(&assignment(Some(i.b), None)), None, None, &set(&[i.b])), vec![A::Load(i.b)]);
}

#[test]
fn dirty_source_is_overwritten_first() {
    let i = ids();
    let w = wc(edited(), Some(i.a));
    assert_eq!(
        output_switch_actions(Some(&assignment(Some(i.b), None)), Some(&w), Some(&HearingProfile::flat()), &set(&[i.a, i.b])),
        vec![A::Overwrite(i.a, edited()), A::Load(i.b)]
    );
}

#[test]
fn clean_source_is_not_overwritten() {
    let i = ids();
    let w = wc(edited(), Some(i.a));
    assert_eq!(
        output_switch_actions(Some(&assignment(Some(i.b), None)), Some(&w), Some(&edited()), &set(&[i.a, i.b])),
        vec![A::Load(i.b)]
    );
}

#[test]
fn unsaved_non_flat_is_saved() {
    let i = ids();
    let w = wc(edited(), None);
    assert_eq!(
        output_switch_actions(Some(&assignment(Some(i.b), None)), Some(&w), None, &set(&[i.b])),
        vec![A::SaveUnsaved(edited()), A::Load(i.b)]
    );
}

#[test]
fn flat_unsaved_is_not_saved() {
    let i = ids();
    let w = wc(HearingProfile::flat(), None);
    assert_eq!(output_switch_actions(Some(&assignment(Some(i.b), None)), Some(&w), None, &set(&[i.b])), vec![A::Load(i.b)]);
}

#[test]
fn same_profile_keeps_edits() {
    let i = ids();
    let w = wc(edited(), Some(i.a));
    let flat = HearingProfile::flat();
    assert_eq!(output_switch_actions(Some(&assignment(Some(i.a), None)), Some(&w), Some(&flat), &set(&[i.a])), vec![]);
    assert_eq!(
        output_switch_actions(Some(&assignment(Some(i.a), Some(LV))), Some(&w), Some(&flat), &set(&[i.a])),
        vec![A::ApplyLevels(LV)]
    );
}

#[test]
fn deleted_assigned_profile_ignored() {
    let i = ids();
    let w = wc(edited(), Some(i.a));
    let flat = HearingProfile::flat();
    assert_eq!(
        output_switch_actions(Some(&assignment(Some(i.b), Some(LV))), Some(&w), Some(&flat), &set(&[i.a])),
        vec![A::ApplyLevels(LV)]
    );
    assert_eq!(output_switch_actions(Some(&assignment(Some(i.b), None)), Some(&w), Some(&flat), &set(&[i.a])), vec![]);
}

#[test]
fn levels_applied_before_load() {
    let i = ids();
    assert_eq!(
        output_switch_actions(Some(&assignment(Some(i.b), Some(LV))), None, None, &set(&[i.b])),
        vec![A::ApplyLevels(LV), A::Load(i.b)]
    );
    let w = wc(edited(), Some(i.a));
    assert_eq!(
        output_switch_actions(Some(&assignment(Some(i.b), Some(LV))), Some(&w), Some(&HearingProfile::flat()), &set(&[i.a, i.b])),
        vec![A::Overwrite(i.a, edited()), A::ApplyLevels(LV), A::Load(i.b)]
    );
}

#[test]
fn prune_clears_deleted() {
    let i = ids();
    let mut a = OutputAssignments::default();
    a.assign(&OutputDevice { uid: "x".into(), name: "X".into() }, Some(i.b), Some(LV));
    a.assign(&OutputDevice { uid: "y".into(), name: "Y".into() }, Some(i.b), None);
    a.prune(&set(&[]));
    assert_eq!(a.assignment("x").unwrap().profile_id, None);
    assert_eq!(a.assignment("x").unwrap().levels, Some(LV));
    assert!(a.assignment("y").is_none());
}

#[test]
fn set_levels_keeps_profile() {
    let i = ids();
    let mut a = OutputAssignments::default();
    a.assign(&OutputDevice { uid: "x".into(), name: "X".into() }, Some(i.b), None);
    a.set_levels(LV, "x");
    assert_eq!(a.assignment("x").unwrap().profile_id, Some(i.b));
    assert_eq!(a.assignment("x").unwrap().levels, Some(LV));
    a.set_levels(LV, "unknown");
    assert!(a.assignment("unknown").is_none());
}
