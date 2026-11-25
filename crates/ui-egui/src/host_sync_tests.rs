//! `selection.changed` and `document.changed`: the host hears of the file in front once a frame, from
//! the frame that saw the change.

use figcraft_engine::Session;
use serde_json::{Value, json};

use crate::present_tests::{Ui, id};

fn heard(u: &mut Ui) -> Vec<(String, Value)> {
    u.app.take_host_events().into_iter().filter(|e| e.name.ends_with(".changed")).map(|e| (e.name, e.data)).collect()
}

fn names(events: &[(String, Value)]) -> Vec<&str> {
    events.iter().map(|(n, _)| n.as_str()).collect()
}

fn setup() -> (Ui, figcraft_doc::NodeId) {
    let mut s = Session::new();
    let r = id(&s.execute("shape.rectangle", &json!({"x": 0, "y": 0, "width": 80, "height": 60})).unwrap());
    s.execute("select.none", &json!({})).unwrap();
    let mut u = Ui::new(s);
    u.frames(2, Vec::new);
    heard(&mut u);
    (u, r)
}

#[test]
fn the_first_frame_with_a_file_tells_the_host_where_things_stand() {
    let mut u = Ui::new(Session::new());
    let first = heard(&mut u);
    assert_eq!(names(&first), ["selection.changed", "document.changed"]);
    assert_eq!(first[0].1["ids"], json!([]));
    assert_eq!(first[1].1["undoDepth"], json!(0));
    u.frames(5, Vec::new);
    assert!(heard(&mut u).is_empty(), "nothing changed, nothing sent");
}

#[test]
fn a_selection_change_sends_only_the_selection() {
    let (mut u, r) = setup();
    u.app.run("select.set", json!({"ids": [r]}));
    u.frames(2, Vec::new);
    let ev = heard(&mut u);
    assert_eq!(names(&ev), ["selection.changed"], "{ev:?}");
    assert_eq!(ev[0].1["ids"], json!([r.0]));
    assert_eq!(ev[0].1["document"], json!(u.app.session.ds().unwrap().uid));
}

#[test]
fn an_edit_sends_the_document_with_the_new_revision_and_undo_depth() {
    let (mut u, r) = setup();
    let before = u.app.session.ds().unwrap().revision;
    u.app.run("node.set", json!({"id": r, "x": 40}));
    u.frames(2, Vec::new);
    let ev = heard(&mut u);
    assert_eq!(names(&ev), ["document.changed"], "{ev:?}");
    assert!(ev[0].1["revision"].as_u64().unwrap() > before);
    assert_eq!(ev[0].1["undoDepth"], json!(u.app.session.undo_depth().0));
    // Two commands in one frame are one event.
    u.app.run("node.set", json!({"id": r, "x": 41}));
    u.app.run("node.set", json!({"id": r, "x": 42}));
    u.frames(2, Vec::new);
    assert_eq!(names(&heard(&mut u)), ["document.changed"]);
    u.app.run("edit.undo", json!({}));
    u.frames(2, Vec::new);
    assert_eq!(names(&heard(&mut u)), ["document.changed"], "undo is a change too");
}

#[test]
fn panning_and_zooming_change_no_document() {
    let (mut u, _) = setup();
    u.app.run("view.zoomIn", json!({}));
    u.app.run("view.pan", json!({"dx": 1, "dy": 0}));
    u.frames(3, Vec::new);
    assert!(heard(&mut u).is_empty());
}

#[test]
fn a_scrub_sends_the_document_on_every_frame_it_changes() {
    let (mut u, r) = setup();
    u.app.run("edit.begin", json!({}));
    for x in [10, 20, 30, 40] {
        u.app.run("edit.preview", json!({"commands": [{"command": "node.set", "params": {"id": r, "x": x}}]}));
        u.frames(1, Vec::new);
    }
    let ev = heard(&mut u);
    assert_eq!(names(&ev), ["document.changed"; 4], "one per previewed frame");
    let revisions: Vec<u64> = ev.iter().map(|(_, d)| d["revision"].as_u64().unwrap()).collect();
    assert!(revisions.windows(2).all(|w| w[0] < w[1]), "{revisions:?}");
    assert!(ev.iter().all(|(_, d)| d["undoDepth"] == ev[0].1["undoDepth"]), "no undo step until the commit");
}

#[test]
fn another_file_in_front_and_no_file_are_told_too() {
    let (mut u, _) = setup();
    let first = u.app.session.ds().unwrap().uid;
    u.app.run("file.new", json!({}));
    u.frames(2, Vec::new);
    let ev = heard(&mut u);
    assert_eq!(names(&ev), ["selection.changed", "document.changed"]);
    assert_ne!(ev[0].1["document"], json!(first));
    u.app.session.docs.clear();
    u.frames(2, Vec::new);
    let ev = heard(&mut u);
    assert_eq!(names(&ev), ["selection.changed", "document.changed"]);
    assert_eq!(ev[1].1["document"], Value::Null);
}
