//! `edit.begin`, `edit.preview`, `edit.commit` and `edit.cancel`: a scrub updates the document at
//! every step and ends as exactly one undo step; cancel puts the value back.

use figcraft_doc::NodeId;
use serde_json::{Value, json};

use crate::Session;
use crate::params::id_of;

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|e| panic!("{command}: {e}"))
}

fn rect(s: &mut Session) -> NodeId {
    id_of(&run(s, "shape.rectangle", json!({"x": 10, "y": 20, "width": 80, "height": 60}))["id"]).unwrap()
}

fn x(s: &Session, id: NodeId) -> f64 {
    s.doc().unwrap().node(id).unwrap().transform.as_coeffs()[4]
}

fn scrub(s: &mut Session, id: NodeId, value: f64) -> Value {
    run(s, "edit.preview", json!({"commands": [{"command": "node.set", "params": {"id": id, "x": value}}]}))
}

#[test]
fn a_scrub_shows_every_step_and_leaves_one_undo_step() {
    let mut s = Session::new();
    let r = rect(&mut s);
    let depth = s.undo_depth().0;
    let revision = s.ds().unwrap().revision;

    run(&mut s, "edit.begin", json!({"label": "Scrub X"}));
    let mut last = revision;
    for v in [11.0, 25.0, 40.0, 33.0, 52.0] {
        scrub(&mut s, r, v);
        assert_eq!(x(&s, r), v, "the document shows the preview at once");
        let now = s.ds().unwrap().revision;
        assert!(now > last, "every preview is a new revision, so the canvas repaints");
        last = now;
        assert_eq!(s.undo_depth().0, depth, "a preview is no undo step");
    }
    assert_eq!(run(&mut s, "edit.commit", json!({})), json!({"committed": true}));
    assert_eq!((x(&s, r), s.undo_depth(), s.undo_labels(1)), (52.0, (depth + 1, 0), vec!["Scrub X".to_string()]));

    run(&mut s, "edit.undo", json!({}));
    assert_eq!(x(&s, r), 10.0, "one undo undoes the whole scrub");
    assert_eq!(s.undo_depth().0, depth);
}

#[test]
fn cancel_puts_the_value_and_selection_back() {
    let mut s = Session::new();
    let r = rect(&mut s);
    run(&mut s, "select.set", json!({"ids": [r]}));
    let depth = s.undo_depth();
    run(&mut s, "edit.begin", json!({}));
    scrub(&mut s, r, 99.0);
    run(&mut s, "select.none", json!({}));
    assert_eq!(run(&mut s, "edit.cancel", json!({})), json!({"cancelled": true}));
    assert_eq!(x(&s, r), 10.0);
    assert_eq!(s.selection(), vec![r]);
    assert_eq!(s.undo_depth(), depth, "no undo step");
    assert!(s.ds().unwrap().interaction.is_none());
    assert_eq!(run(&mut s, "edit.cancel", json!({})), json!({"cancelled": false}), "cancelling again is harmless");
}

#[test]
fn a_relative_edit_does_not_pile_up_across_previews() {
    let mut s = Session::new();
    let r = rect(&mut s);
    run(&mut s, "edit.begin", json!({}));
    for _ in 0..3 {
        run(&mut s, "edit.preview", json!({"commands": [{"command": "transform.move", "params": {"ids": [r], "dx": 5, "dy": 0}}]}));
    }
    assert_eq!(x(&s, r), 15.0, "each preview starts from where the gesture began");
    run(&mut s, "edit.commit", json!({}));
    assert_eq!(x(&s, r), 15.0);
}

#[test]
fn a_failing_preview_keeps_the_last_good_one() {
    let mut s = Session::new();
    let r = rect(&mut s);
    run(&mut s, "edit.begin", json!({}));
    scrub(&mut s, r, 30.0);
    let bad = s.execute(
        "edit.preview",
        &json!({"commands": [{"command": "node.set", "params": {"id": r, "x": 77}}, {"command": "node.set", "params": {"id": r, "visible": "nope"}}]}),
    );
    assert!(bad.is_err());
    assert_eq!(x(&s, r), 30.0, "the first step of the failed preview is not left behind");
    assert!(s.ds().unwrap().interaction.is_some(), "the gesture goes on");
    run(&mut s, "edit.commit", json!({}));
    assert_eq!(x(&s, r), 30.0);
}

#[test]
fn nothing_changed_is_no_step() {
    let mut s = Session::new();
    let r = rect(&mut s);
    let depth = s.undo_depth();
    run(&mut s, "edit.begin", json!({}));
    scrub(&mut s, r, 70.0);
    scrub(&mut s, r, 10.0);
    assert_eq!(run(&mut s, "edit.commit", json!({})), json!({"committed": false}));
    assert_eq!(s.undo_depth(), depth);
    assert_eq!(run(&mut s, "edit.commit", json!({})), json!({"committed": false}), "committing with no gesture is harmless");
}

#[test]
fn the_gesture_commands_refuse_what_would_break_a_gesture() {
    let mut s = Session::new();
    let r = rect(&mut s);
    let err = |s: &mut Session, c: &str, p: Value| s.execute(c, &p).unwrap_err().to_string();
    assert!(err(&mut s, "edit.preview", json!({"commands": []})).contains("no gesture"));
    run(&mut s, "edit.begin", json!({}));
    assert!(err(&mut s, "edit.begin", json!({})).contains("already in progress"));
    for c in ["edit.begin", "edit.commit", "edit.cancel", "edit.undo", "file.close", "doc.nextTab"] {
        assert!(err(&mut s, "edit.preview", json!({"commands": [{"command": c, "params": {}}]})).contains("can't run inside a gesture"), "{c}");
    }
    assert!(err(&mut s, "edit.undo", json!({})).contains("finish the gesture"));
    assert!(err(&mut s, "edit.redo", json!({})).contains("finish the gesture"));
    assert!(err(&mut s, "edit.preview", json!({"commands": [{"command": "node.set", "args": {}}]})).contains("args"), "a misspelt step key is named");
    scrub(&mut s, r, 12.0);
    run(&mut s, "edit.cancel", json!({}));
    assert_eq!(run(&mut s, "edit.undo", json!({})), json!({"undone": true}), "undo works again once it ends (it takes the rectangle back)");
}

#[test]
fn a_preview_can_run_a_batch_of_commands_with_references() {
    let mut s = Session::new();
    let r = rect(&mut s);
    run(&mut s, "edit.begin", json!({"label": "Resize"}));
    run(
        &mut s,
        "edit.preview",
        json!({"commands": [
            {"command": "node.set", "params": {"id": r, "width": 120}},
            {"command": "node.set", "params": {"id": r, "height": 30}},
        ]}),
    );
    run(&mut s, "edit.commit", json!({}));
    let n = s.doc().unwrap().node(r).unwrap();
    assert_eq!((n.width, n.height), (120.0, 30.0));
    assert_eq!(s.undo_labels(1), vec!["Resize".to_string()], "two edits, one step");
}
