//! The header menu of saved profile versions (Swift `ProfileMenu`): load, Save, Save As…,
//! Rename…, Duplicate, Use on <output> and Delete…, with the name prompt and the delete confirmation as modals.

use std::time::Instant;

use egui::containers::menu::MenuButton;
use egui::{
    Align, Button, Id, Key, KeyboardShortcut, Layout, Modal, Modifiers, RichText, TextEdit, Ui,
};
use uuid::Uuid;

use crate::i18n::{tr, trf};
use crate::model::app::AppModel;
use crate::ui::widgets::{MODAL_WIDTH, ModalAnswer, confirm_modal};

#[derive(Clone, Debug, PartialEq)]
enum Dialog {
    /// The name prompt of Save As… or Rename….
    Prompt {
        kind: PromptKind,
        name: String,
    },
    ConfirmDelete(Uuid),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum PromptKind {
    SaveAs,
    Rename(Uuid),
}

fn dialog_id() -> Id {
    Id::unique("profile_menu_dialog")
}

pub fn show(ui: &mut Ui, m: &mut AppModel, now: Instant) {
    let e = &m.editor;
    let current = e.current_name().map(str::to_owned);
    let has_source = current.is_some();
    let dirty = e.is_dirty();
    let mut label = current.clone().unwrap_or_else(|| tr("Unsaved profile"));
    if dirty {
        label.push_str(" •");
    }
    let saved: Vec<(Uuid, String)> = e.saved.iter().map(|s| (s.id, s.name.clone())).collect();
    let source = e.source_id;
    let mut dialog = None;

    let can_save = !has_source || dirty;
    let shortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
    let no_dialog = ui.data(|d| d.get_temp::<Dialog>(dialog_id()).is_none());
    if no_dialog && can_save && ui.input_mut(|i| i.consume_shortcut(&shortcut)) {
        dialog = save(m);
    }

    // Truncated, so a long name stays within the plot column's header.
    MenuButton::from_button(Button::new(label).truncate()).ui(ui, |ui| {
        for (id, name) in &saved {
            if ui
                .add(Button::selectable(source == Some(*id), name.as_str()))
                .clicked()
            {
                m.load_profile(*id, now);
            }
        }
        ui.separator();
        let save_button =
            Button::new(tr("Save")).shortcut_text(ui.ctx().format_shortcut(&shortcut));
        if ui.add_enabled(can_save, save_button).clicked() {
            dialog = save(m);
        }
        if ui.button(tr("Save As…")).clicked() {
            dialog = Some(save_as_prompt(current.as_deref()));
        }
        if ui
            .add_enabled(has_source, Button::new(tr("Rename…")))
            .clicked()
            && let Some(id) = source
        {
            dialog = Some(Dialog::Prompt {
                kind: PromptKind::Rename(id),
                name: current.clone().unwrap_or_default(),
            });
        }
        if ui
            .add_enabled(has_source, Button::new(tr("Duplicate")))
            .clicked()
            && let Some(id) = source
        {
            m.editor.duplicate(id);
        }
        use_on_current_output(ui, m);
        if ui
            .add_enabled(has_source, Button::new(tr("Delete…")))
            .clicked()
            && let Some(id) = source
        {
            dialog = Some(Dialog::ConfirmDelete(id));
        }
    });

    if let Some(d) = dialog {
        ui.data_mut(|data| data.insert_temp(dialog_id(), d));
    }
    show_dialog(ui, m);
}

/// "Use on <output>": assigns the current version to the output in use now, or removes the
/// assignment when it already is (Swift `assignCurrentOutput`). Checked while assigned.
fn use_on_current_output(ui: &mut Ui, m: &mut AppModel) {
    let source = m.editor.source_id;
    let current = m.current_output.clone();
    let title = current.as_ref().map_or_else(
        || tr("Use on current output"),
        |d| trf("Use on %@", &[&d.name]),
    );
    let assigned = match (source, &current) {
        (Some(id), Some(d)) => m
            .assignments
            .assignment(&d.uid)
            .is_some_and(|a| a.profile_id == Some(id)),
        _ => false,
    };
    let enabled = source.is_some() && current.is_some();
    if ui
        .add_enabled(enabled, Button::selectable(assigned, title))
        .clicked()
        && let Some(d) = current
    {
        m.assign(&d, if assigned { None } else { source });
    }
}

/// Overwrites the current version, or asks for a name when there is none.
fn save(m: &mut AppModel) -> Option<Dialog> {
    if m.editor.save() {
        return None;
    }
    let current = m.editor.current_name().map(str::to_owned);
    Some(save_as_prompt(current.as_deref()))
}

/// Suggests a fresh name: re-using the current one would be rejected as taken.
fn save_as_prompt(current: Option<&str>) -> Dialog {
    Dialog::Prompt {
        kind: PromptKind::SaveAs,
        name: current.map_or_else(|| tr("My profile"), |n| trf("%@ new", &[n])),
    }
}

fn show_dialog(ui: &mut Ui, m: &mut AppModel) {
    let Some(dialog) = ui.data(|d| d.get_temp::<Dialog>(dialog_id())) else {
        return;
    };
    let ctx = ui.ctx().clone();
    let next = match dialog {
        Dialog::Prompt { kind, mut name } => match prompt(&ctx, kind, &mut name) {
            ModalAnswer::Open => Some(Dialog::Prompt { kind, name }),
            ModalAnswer::Cancelled => None,
            ModalAnswer::Confirmed => {
                match kind {
                    PromptKind::SaveAs => m.editor.save_as(&name),
                    PromptKind::Rename(id) => m.editor.rename(id, &name),
                }
                None
            }
        },
        Dialog::ConfirmDelete(id) => {
            let name = m
                .editor
                .saved
                .iter()
                .find(|s| s.id == id)
                .map_or(String::new(), |s| s.name.clone());
            match confirm_modal(
                &ctx,
                dialog_id().with("delete"),
                &trf("Delete “%@”?", &[&name]),
                None,
                &tr("Delete"),
            ) {
                ModalAnswer::Open => Some(Dialog::ConfirmDelete(id)),
                ModalAnswer::Cancelled => None,
                ModalAnswer::Confirmed => {
                    m.editor.delete(id);
                    None
                }
            }
        }
    };
    ui.data_mut(|d| match next {
        Some(n) => {
            d.insert_temp(dialog_id(), n);
        }
        None => {
            d.remove::<Dialog>(dialog_id());
        }
    });
}

/// The name prompt: title, the name field (focused), Cancel and Save or Rename. Return submits.
fn prompt(ctx: &egui::Context, kind: PromptKind, name: &mut String) -> ModalAnswer {
    let (title, button) = match kind {
        PromptKind::SaveAs => (tr("Save As"), tr("Save")),
        PromptKind::Rename(_) => (tr("Rename"), tr("Rename")),
    };
    let r = Modal::new(dialog_id().with("prompt")).show(ctx, |ui| {
        ui.set_width(MODAL_WIDTH);
        ui.label(RichText::new(title).strong());
        let field = ui
            .add(
                TextEdit::singleline(name)
                    .hint_text(tr("Name"))
                    .desired_width(f32::INFINITY),
            )
            .accessible_name(tr("Name"));
        if ui.memory(|mem| mem.focused().is_none()) {
            field.request_focus();
        }
        let submitted = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        ui.add_space(6.0);
        let mut answer = ModalAnswer::Open;
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button(button).clicked() || submitted {
                answer = ModalAnswer::Confirmed;
            }
            if ui.button(tr("Cancel")).clicked() {
                answer = ModalAnswer::Cancelled;
            }
        });
        answer
    });
    if r.inner == ModalAnswer::Open && r.should_close() {
        ModalAnswer::Cancelled
    } else {
        r.inner
    }
}
