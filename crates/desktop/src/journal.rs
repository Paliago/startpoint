use crate::{
    components::*,
    editor::EditorSection,
    icons,
    state::{Overlay, State},
};
use rune::{El, ScrollSpec, TextEdit, col, el, row};
use rune_kit::{ButtonVariant, Kit, Markdown, SegmentedControl, TextArea};
use startpoint_sdk::{Command, GameId};

impl State {
    pub fn open_journal(&mut self, id: GameId) {
        if self.game(&id).is_none() {
            return;
        }
        if self.editor.game_id.as_ref() != Some(&id) {
            self.open_editor(id, EditorSection::Details);
        }
        self.show_overlay(Overlay::Journal);
    }
}

pub fn timestamp(value: u64) -> String {
    i64::try_from(value)
        .ok()
        .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0))
        .map(|date| {
            date.with_timezone(&chrono::Local)
                .format("%d %b %Y · %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "Date unavailable".into())
}

fn format_entry(state: &mut State, prefix: &str, suffix: &str) {
    let edit = &mut state.editor.journal;
    let start = edit.anchor.min(edit.focus);
    let selected = if edit.selected().is_empty() {
        "text".to_owned()
    } else {
        edit.selected().to_owned()
    };
    edit.insert(&format!("{prefix}{selected}{suffix}"));
    edit.anchor = start + prefix.len();
    edit.focus = edit.anchor + selected.len();
    edit.focused = true;
    state.editor.journal_preview = false;
}

fn composer(state: &State, kit: &Kit, busy: bool) -> El {
    let mut formats = row().w_full().gap(4.0).center_y();
    for (title, prefix, suffix) in [
        ("Bold", "**", "**"),
        ("Italic", "_", "_"),
        ("Code", "`", "`"),
        ("List", "\n- ", ""),
        ("Quote", "\n> ", ""),
    ] {
        formats = formats.child(
            button(title)
                .variant(ButtonVariant::Ghost)
                .disabled(busy)
                .on_press(move |s: &mut State| format_entry(s, prefix, suffix))
                .build(kit)
                .h(28.0)
                .px(8.0),
        );
    }
    let input = if state.editor.journal_preview {
        col()
            .w_full()
            .min_h(150.0)
            .padding(14.0)
            .child(Markdown::new(&state.editor.journal.text).build(kit))
    } else {
        TextArea::new(&state.editor.journal)
            .label("Journal entry")
            .placeholder("Write about this session… Markdown is supported.")
            .width(None)
            .rows(6)
            .on_edit(|s: &mut State| &mut s.editor.journal)
            .on_submit(|s: &mut State, _| s.save_journal())
            .on_key(|s: &mut State, event, _| {
                if event.is_command('b') {
                    format_entry(s, "**", "**");
                    return true;
                }
                if event.is_command('i') {
                    format_entry(s, "_", "_");
                    return true;
                }
                false
            })
            .build(kit)
            .key("journal-text")
            .h(170.0)
            .radius(8.0)
            .disabled(busy)
    };
    col()
        .key("journal-composer")
        .w_full()
        .gap(12.0)
        .child(
            row()
                .w_full()
                .justify_between()
                .center_y()
                .child(label(
                    if state.editor.journal_entry.is_some() {
                        "Edit entry"
                    } else {
                        "New entry"
                    },
                    14.0,
                    550.0,
                    kit,
                ))
                .child(
                    button(if state.editor.journal_preview {
                        "Write"
                    } else {
                        "Preview"
                    })
                    .variant(ButtonVariant::Ghost)
                    .on_press(|s: &mut State| s.editor.journal_preview = !s.editor.journal_preview)
                    .build(kit)
                    .key("journal-preview"),
                ),
        )
        .child(
            col()
                .w_full()
                .padding(10.0)
                .gap(8.0)
                .radius(12.0)
                .border(0.5, kit.c.border_base)
                .fill(kit.c.layer_02)
                .child(formats)
                .child(input),
        )
        .child(
            row()
                .w_full()
                .gap(10.0)
                .center_y()
                .child(
                    primary(if state.editor.journal_entry.is_some() {
                        "Save changes"
                    } else {
                        "Add entry"
                    })
                    .disabled(busy || state.editor.journal.text.trim().is_empty())
                    .on_press(|s: &mut State| s.save_journal())
                    .build(kit)
                    .key("save-journal"),
                )
                .child(
                    button("New entry")
                        .disabled(busy)
                        .on_press(|s: &mut State| {
                            s.editor.journal_entry = None;
                            s.editor.journal = TextEdit::new("");
                            s.editor.journal_preview = false;
                        })
                        .build(kit)
                        .key("journal-new"),
                )
                .child(el().grow(1.0))
                .child(label("⌘ / Ctrl + Enter to save", 10.0, 400.0, kit).color(kit.c.text_muted)),
        )
}

pub fn view(state: &State, kit: &Kit) -> El {
    if state.editor.journal_tasks_open {
        return pane(state, kit, crate::tasks::view(state, kit));
    }
    let busy = state
        .editor
        .journal_job
        .is_some_and(|id| state.job_active(id));
    let mut feed = col()
        .key("journal-feed")
        .w_full()
        .gap(24.0)
        .child(composer(state, kit, busy));
    let entries: Vec<_> = state
        .snapshot
        .journal
        .iter()
        .filter(|entry| Some(&entry.game_id) == state.editor.game_id.as_ref())
        .collect();
    if entries.is_empty() {
        feed = feed.child(hint("Your journal starts here. Add a note after a session, then return to this dated history whenever you need it.", kit));
    }
    for entry in entries {
        let edit_id = entry.id.clone();
        let text = entry.text.clone();
        let delete_id = entry.id.clone();
        feed = feed.child(
            col()
                .key(format!("journal-entry-{}", entry.id))
                .w_full()
                .gap(12.0)
                .pt(20.0)
                .child(el().w_full().h(1.0).fill(kit.c.border_base))
                .child(
                    row()
                        .w_full()
                        .gap(8.0)
                        .center_y()
                        .child(
                            col()
                                .grow(1.0)
                                .min_w(0.0)
                                .gap(4.0)
                                .child(label(timestamp(entry.created_at), 12.0, 550.0, kit))
                                .child(
                                    label(
                                        entry
                                            .author
                                            .as_deref()
                                            .unwrap_or("Local entry · author not recorded"),
                                        11.0,
                                        400.0,
                                        kit,
                                    )
                                    .color(kit.c.text_muted),
                                ),
                        )
                        .child(
                            toolbar_icon(&icons::PEN, "Edit entry", kit)
                                .disabled(busy)
                                .on_press_tx(move |s: &mut State, tx| {
                                    s.editor.journal_entry = Some(edit_id.clone());
                                    s.editor.journal_preview = false;
                                    s.editor.journal = TextEdit::new(text.clone());
                                    s.editor.journal.focused = true;
                                    tx.scroll_into_view("journal-composer");
                                })
                                .key(format!("journal-edit-{}", entry.id)),
                        )
                        .child(
                            toolbar_icon(&icons::TRASH, "Delete entry", kit)
                                .disabled(busy)
                                .on_press(move |s: &mut State| {
                                    if let Some(game_id) = s.editor.game_id.clone() {
                                        s.confirm(
                                            "Delete journal entry?",
                                            "This entry will be permanently removed.",
                                            Command::DeleteJournalEntry {
                                                game_id,
                                                entry_id: delete_id.clone(),
                                            },
                                        );
                                    }
                                })
                                .key(format!("journal-delete-{}", entry.id)),
                        ),
                )
                .child(
                    Markdown::new(&entry.text)
                        .build(kit)
                        .key(format!("journal-body-{}", entry.id)),
                ),
        );
    }
    pane(state, kit, feed)
}

fn pane(state: &State, kit: &Kit, content: El) -> El {
    col()
        .key("game-journal")
        .w_full()
        .h_full()
        .gap(16.0)
        .child(
            SegmentedControl::new(
                &["Journal", "To-dos"],
                Some(usize::from(state.editor.journal_tasks_open)),
            )
            .width(None)
            .on_pick(|s: &mut State, index, _| s.editor.journal_tasks_open = index == 1)
            .build(kit)
            .key("journal-tabs"),
        )
        .child(
            col()
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(content),
        )
}
