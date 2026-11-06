use crate::{components::*, icons, state::State};
use rune::{El, TextEdit, col, el, row};
use rune_kit::{Checkbox, Kit, TextInput};
use startpoint_sdk::Command;

impl State {
    pub fn save_task(&mut self) {
        if self.editor.task_job.is_some_and(|id| self.job_active(id)) {
            return;
        }
        let Some(id) = self.editor.game_id.clone() else {
            return;
        };
        if self.editor.task.text.trim().is_empty() {
            return;
        }
        let completed = self
            .game(&id)
            .and_then(|game| {
                game.tasks
                    .iter()
                    .find(|task| Some(&task.id) == self.editor.task_edit.as_ref())
            })
            .is_some_and(|task| task.completed);
        self.editor.task_job = self.submit(Command::SaveGameTask {
            game_id: id,
            task_id: self.editor.task_edit.clone(),
            text: self.editor.task.text.clone(),
            completed,
        });
    }
}

pub fn view(state: &State, kit: &Kit) -> El {
    let Some(game) = state.editor.game_id.as_ref().and_then(|id| state.game(id)) else {
        return hint("This game is no longer available.", kit);
    };
    let busy = state.editor.task_job.is_some_and(|id| state.job_active(id));
    let completed = game.tasks.iter().filter(|task| task.completed).count();
    let mut content = col().key("game-tasks").w_full().gap(16.0)
        .child(hint("A separate checklist for this game. Tasks stay here independently of your journal entries.", kit))
        .child(row().w_full().gap(10.0).center_y()
            .child(TextInput::new(&state.editor.task).label("Task").placeholder("Add a goal or reminder…").width(None).height(36.0)
                .on_edit(|s: &mut State| &mut s.editor.task).on_submit(|s: &mut State, _| s.save_task())
                .build(kit).key("task-text").grow(1.0).min_w(0.0).disabled(busy))
            .child(primary(if state.editor.task_edit.is_some() { "Save task" } else { "Add task" })
                .disabled(busy || state.editor.task.text.trim().is_empty()).on_press(|s: &mut State| s.save_task()).build(kit).key("task-save")))
        .child(label(format!("{completed} of {} completed", game.tasks.len()), 11.0, 450.0, kit).color(kit.c.text_muted));
    if state.editor.task_edit.is_some() {
        content = content.child(
            button("Cancel edit")
                .on_press(|s: &mut State| {
                    s.editor.task_edit = None;
                    s.editor.task = TextEdit::new("");
                })
                .build(kit)
                .key("task-cancel"),
        );
    }
    for task in &game.tasks {
        let game_id = game.id.clone();
        let task_id = task.id.clone();
        let text = task.text.clone();
        let checked = task.completed;
        let edit_id = task.id.clone();
        let edit_text = task.text.clone();
        let delete_game = game.id.clone();
        let delete_id = task.id.clone();
        content = content.child(
            col()
                .key(format!("task-{}", task.id))
                .w_full()
                .gap(10.0)
                .child(el().w_full().h(1.0).fill(kit.c.border_base))
                .child(
                    row()
                        .w_full()
                        .gap(12.0)
                        .center_y()
                        .child(
                            Checkbox::new(checked)
                                .label(&task.text)
                                .hide_label()
                                .disabled(busy)
                                .on_toggle(move |s: &mut State| {
                                    s.submit(Command::SaveGameTask {
                                        game_id: game_id.clone(),
                                        task_id: Some(task_id.clone()),
                                        text: text.clone(),
                                        completed: !checked,
                                    });
                                })
                                .build(kit)
                                .key(format!("task-check-{}", task.id)),
                        )
                        .child(
                            label(&task.text, 13.0, 450.0, kit)
                                .color(if checked {
                                    kit.c.text_muted
                                } else {
                                    kit.c.text_base
                                })
                                .wrap()
                                .grow(1.0)
                                .min_w(0.0),
                        )
                        .child(
                            toolbar_icon(&icons::PEN, "Edit task", kit)
                                .disabled(busy)
                                .on_press(move |s: &mut State| {
                                    s.editor.task_edit = Some(edit_id.clone());
                                    s.editor.task = TextEdit::new(edit_text.clone());
                                    s.editor.task.focused = true;
                                })
                                .key(format!("task-edit-{}", task.id)),
                        )
                        .child(
                            toolbar_icon(&icons::TRASH, "Delete task", kit)
                                .disabled(busy)
                                .on_press(move |s: &mut State| {
                                    if s.editor.task_edit.as_ref() == Some(&delete_id) {
                                        s.editor.task_edit = None;
                                        s.editor.task = TextEdit::new("");
                                    }
                                    s.submit(Command::DeleteGameTask {
                                        game_id: delete_game.clone(),
                                        task_id: delete_id.clone(),
                                    });
                                })
                                .key(format!("task-delete-{}", task.id)),
                        ),
                ),
        );
    }
    if game.tasks.is_empty() {
        content = content.child(hint(
            "No tasks yet. Add something you want to do next time you play.",
            kit,
        ));
    }
    content
}
