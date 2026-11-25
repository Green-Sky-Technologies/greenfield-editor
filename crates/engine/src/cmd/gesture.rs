//! Gestures as commands: a scrub, a drag or a slider is many small edits that end as one undo step.
//!
//! `edit.begin` starts one, `edit.preview` shows what the edits would do (the document is put back
//! to where the gesture began and the commands run again, so a relative edit never piles up),
//! `edit.commit` makes the gesture one undo step and `edit.cancel` puts the document back. They are
//! [`Session::begin_interaction`], [`Session::preview`]'s way of working, [`Session::commit_interaction`]
//! and [`Session::cancel_interaction`], which the editor's own panels already use.

use std::sync::Arc;

use serde_json::{Value, json};

use super::CommandSpec;
use crate::params::string;
use crate::{BatchScope, BatchStep, EngineError, Result, Session, bad, spec};

/// What a preview may not run: it would end or outlive the gesture, or edit something else.
fn refuse_in_preview(command: &str) -> Result<()> {
    let denied = command.starts_with("edit.begin")
        || matches!(command, "edit.preview" | "edit.commit" | "edit.cancel" | "edit.undo" | "edit.redo" | "edit.repeat")
        || command.starts_with("file.")
        || command.starts_with("doc.")
        || command.starts_with("window.");
    if denied { Err(bad("edit.preview", format!("`{command}` can't run inside a gesture"))) } else { Ok(()) }
}

/// Undo and redo during a gesture of `edit.begin` would swap the document under it: refuse them
/// until it ends. (The editor's own gestures, the pen's anchors among them, have their own rules.)
pub(crate) fn refuse_during_gesture(s: &Session, id: &str) -> Result<()> {
    if s.gesture_is_host() {
        return Err(EngineError::Disabled(id.to_string(), "finish the gesture first (edit.commit or edit.cancel)".into()));
    }
    Ok(())
}

impl Session {
    /// The interaction open on the document in front was started with `edit.begin` (a host's
    /// gesture), not by the canvas's own drag.
    pub fn gesture_is_host(&self) -> bool {
        self.ds().is_ok_and(|d| self.gesture_cmd == Some(d.uid) && d.interaction.is_some())
    }
}

fn begin(s: &mut Session, p: &Value) -> Result<Value> {
    if let Ok(ds) = s.ds()
        && let Some(it) = &ds.interaction
    {
        return Err(bad("edit.begin", format!("a gesture (\"{}\") is already in progress: edit.commit or edit.cancel it first", it.label)));
    }
    let label = string(p, "label").unwrap_or_else(|| "Edit".into());
    s.begin_interaction(&label)?;
    s.gesture_cmd = s.ds().ok().map(|d| d.uid);
    Ok(json!({"label": label}))
}

fn preview(s: &mut Session, p: &Value) -> Result<Value> {
    let steps: Vec<BatchStep> = match p.get("commands") {
        Some(Value::Array(a)) => {
            let mut out = Vec::with_capacity(a.len());
            for (i, c) in a.iter().enumerate() {
                let cmd = c.get("command").and_then(Value::as_str).ok_or_else(|| bad("edit.preview", format!("step {i} has no `command`")))?;
                crate::batch::refuse_unknown_step_keys(i, c)?;
                refuse_in_preview(cmd)?;
                out.push(BatchStep { name: crate::batch::step_name(i, c)?, ..BatchStep::new(cmd, c.get("params").cloned().unwrap_or(json!({}))) });
            }
            out
        }
        _ => return Err(bad("edit.preview", "`commands` must be an array")),
    };
    let mut scope = BatchScope::new(&steps)?;
    // From the gesture's start each time; a failed step goes back to the last preview that worked.
    let (shown_doc, shown_sel) = {
        let ds = s.ds_mut()?;
        let Some(it) = ds.interaction.as_mut() else { return Err(bad("edit.preview", "no gesture in progress: call edit.begin first")) };
        it.moved_by = None;
        let (doc, sel) = (it.doc.clone(), it.selection.clone());
        let shown = (std::mem::replace(&mut ds.doc, doc), std::mem::replace(&mut ds.selection, sel));
        // The document under the commands is not the one a canvas pinned to the last preview shows.
        ds.revision += 1;
        shown
    };
    for step in &steps {
        if let Err(e) = s.batch_step(&mut scope, &step.command, &step.params) {
            let ds = s.ds_mut()?;
            ds.doc = shown_doc;
            ds.selection = shown_sel;
            ds.revision += 1;
            return Err(e);
        }
    }
    Ok(Value::Array(scope.into_results()))
}

fn commit(s: &mut Session, _: &Value) -> Result<Value> {
    let ds = s.ds()?;
    // One step, or none when the previews ended where the gesture began.
    let changed = ds.interaction.as_ref().is_some_and(|it| !Arc::ptr_eq(&it.doc, &ds.doc) && *it.doc != *ds.doc);
    s.commit_interaction()?;
    s.gesture_cmd = None;
    Ok(json!({"committed": changed}))
}

fn cancel(s: &mut Session, _: &Value) -> Result<Value> {
    let was = s.ds()?.interaction.is_some();
    s.cancel_interaction()?;
    s.gesture_cmd = None;
    Ok(json!({"cancelled": was}))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        spec!(
            "edit.begin",
            "Begin a gesture",
            "{label? (the undo step's name; default \"Edit\")}  start a drag or scrub: edit.preview changes the document without undo steps, edit.commit makes them one step, edit.cancel puts everything back. One gesture at a time; edit.undo and edit.redo wait until it ends",
            false,
            begin
        ),
        spec!(
            "edit.preview",
            "Preview a gesture",
            "{commands: [{command, params, as?}]}  show the commands' result on the canvas from where the gesture began (the document is reset first, so a relative edit doesn't add up; $step and $ref work as in command.batch). A failing step undoes the whole preview and keeps the last good one",
            false,
            preview
        ),
        spec!("edit.commit", "Commit a gesture", "{}  the gesture's previews become one undo step (none if nothing changed)", false, commit),
        spec!("edit.cancel", "Cancel a gesture", "{}  the document and selection go back to where edit.begin found them", false, cancel),
    ]
}
