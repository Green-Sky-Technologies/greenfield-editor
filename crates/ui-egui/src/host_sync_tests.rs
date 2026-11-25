//! The host hears of the editor once a frame, from the frame that saw the change: `selection.changed`
//! and `document.changed` with the layers' fields, `tool.changed`, `text.editing`, `viewport.changed`
//! and `interaction.changed`.

use figcraft_engine::Session;
use serde_json::{Value, json};

use crate::present_tests::{Ui, id};

/// Every event since the last call.
fn heard_all(u: &mut Ui) -> Vec<(String, Value)> {
    u.app.take_host_events().into_iter().map(|e| (e.name, e.data)).collect()
}

/// `selection.changed` and `document.changed` since the last call.
fn heard(u: &mut Ui) -> Vec<(String, Value)> {
    heard_all(u).into_iter().filter(|(n, _)| n == "selection.changed" || n == "document.changed").collect()
}

/// The one `document.changed` since the last call.
fn doc_change(u: &mut Ui) -> Value {
    let ev: Vec<_> = heard(u).into_iter().filter(|(n, _)| n == "document.changed").collect();
    assert_eq!(ev.len(), 1, "{ev:?}");
    ev.into_iter().next().map(|(_, d)| d).unwrap_or_default()
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
    heard_all(&mut u);
    (u, r)
}

#[test]
fn the_first_frame_with_a_file_tells_the_host_where_things_stand() {
    let mut u = Ui::new(Session::new());
    let first = heard(&mut u);
    assert_eq!(names(&first), ["selection.changed", "document.changed"]);
    assert_eq!(first[0].1["ids"], json!([]));
    assert_eq!(first[1].1["undoDepth"], json!(0));
    assert_eq!(first[1].1["resync"], json!(true), "no patch for a file the host hasn't seen");
    u.frames(5, Vec::new);
    assert!(heard_all(&mut u).is_empty(), "nothing changed, nothing sent");
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
    assert!(heard_all(&mut u).iter().all(|(n, _)| n == "viewport.changed"));
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

#[test]
fn a_selection_carries_the_selected_layers_fields() {
    let (mut u, r) = setup();
    u.app.run("node.set", json!({"id": r, "name": "Card", "fill": "#ff9a4d"}));
    u.frames(1, Vec::new);
    heard_all(&mut u);
    u.app.run("select.set", json!({"ids": [r]}));
    u.frames(2, Vec::new);
    let ev = heard(&mut u);
    let nodes = ev[0].1["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 1);
    let expected = figcraft_engine::inspect::node_json(&u.app.session.ds().unwrap().doc, r, 0);
    assert_eq!(nodes[0], expected, "node.get's fields at depth 0");
    assert_eq!((nodes[0]["name"].clone(), nodes[0]["type"].clone(), nodes[0]["width"].clone()), (json!("Card"), json!("RECTANGLE"), json!(80.0)));
    assert!(nodes[0]["fills"].is_array());
    u.app.run("select.none", json!({}));
    u.frames(2, Vec::new);
    assert_eq!(heard(&mut u)[0].1["nodes"], json!([]));
}

#[test]
fn an_edit_carries_only_the_fields_that_changed() {
    let (mut u, r) = setup();
    u.app.run("node.set", json!({"id": r, "x": 40, "opacity": 0.5}));
    u.frames(2, Vec::new);
    let d = doc_change(&mut u);
    assert_eq!(d["resync"], json!(false));
    let bounds = json!({"x": 40.0, "y": 0.0, "width": 80.0, "height": 60.0});
    assert_eq!(d["nodes"], json!([{"id": r.0, "x": 40.0, "absoluteBounds": bounds, "opacity": 0.5}]));
    assert_eq!((d["added"].clone(), d["removed"].clone(), d["moved"].clone()), (json!([]), json!([]), json!([])));
    u.app.run("node.set", json!({"id": r, "opacity": 1}));
    u.frames(2, Vec::new);
    assert_eq!(doc_change(&mut u)["nodes"], json!([{"id": r.0, "opacity": null}]), "a field back at its default is null");
}

#[test]
fn added_and_removed_layers_say_where_they_are_and_were() {
    let (mut u, r) = setup();
    let page = u.app.session.ds().unwrap().page;
    let s = id(&u.app.session.execute("shape.rectangle", &json!({"x": 100, "y": 0, "width": 10, "height": 10})).unwrap());
    u.frames(2, Vec::new);
    let d = doc_change(&mut u);
    assert_eq!(d["added"], json!([{"id": s.0, "parent": page.0, "index": 1}]));
    let added = d["nodes"].as_array().unwrap().iter().find(|n| n["id"] == json!(s.0)).cloned().unwrap();
    assert_eq!(added, figcraft_engine::inspect::node_json(&u.app.session.ds().unwrap().doc, s, 0), "an added layer comes with every field");
    assert!(d["nodes"].as_array().unwrap().iter().any(|n| n["id"] == json!(page.0) && n["childCount"] == json!(2)), "{d}");
    u.app.run("edit.delete", json!({"ids": [r]}));
    u.frames(2, Vec::new);
    let d = doc_change(&mut u);
    assert_eq!(d["removed"], json!([{"id": r.0, "parent": page.0, "index": 0}]));
    assert_eq!(d["moved"], json!([]), "the layer after it keeps its order");
}

#[test]
fn a_reorder_moves_only_the_layer_that_moved() {
    let (mut u, a) = setup();
    let page = u.app.session.ds().unwrap().page;
    let mut more = Vec::new();
    for x in [100, 200, 300] {
        more.push(id(&u.app.session.execute("shape.rectangle", &json!({"x": x, "y": 0, "width": 10, "height": 10})).unwrap()));
    }
    u.frames(2, Vec::new);
    heard_all(&mut u);
    u.app.run("arrange.front", json!({"ids": [a]}));
    u.frames(2, Vec::new);
    let d = doc_change(&mut u);
    assert_eq!(d["moved"], json!([{"id": a.0, "parent": page.0, "index": 3}]), "not the three that shifted down: {d}");
    assert_eq!(d["added"], json!([]));
}

#[test]
fn grouping_moves_the_layers_into_the_new_group() {
    let (mut u, a) = setup();
    let b = id(&u.app.session.execute("shape.rectangle", &json!({"x": 100, "y": 0, "width": 10, "height": 10})).unwrap());
    u.frames(2, Vec::new);
    heard_all(&mut u);
    u.app.run("select.set", json!({"ids": [a, b]}));
    u.app.run("object.group", json!({}));
    u.frames(2, Vec::new);
    let d = doc_change(&mut u);
    let group = d["added"][0]["id"].as_u64().unwrap();
    assert_eq!(d["added"].as_array().unwrap().len(), 1, "{d}");
    assert_eq!(d["moved"], json!([{"id": a.0, "parent": group, "index": 0}, {"id": b.0, "parent": group, "index": 1}]));
}

#[test]
fn too_big_a_change_asks_the_host_to_read_the_tree_again() {
    let (mut u, _) = setup();
    let steps: Vec<Value> = (0..=super::app::host_sync::MAX_PATCH)
        .map(|i| json!({"command": "shape.rectangle", "params": {"x": i, "y": 0, "width": 1, "height": 1}}))
        .collect();
    u.app.run("command.batch", json!({"commands": steps}));
    u.frames(2, Vec::new);
    let d = doc_change(&mut u);
    assert_eq!(d["resync"], json!(true));
    assert_eq!(d["nodes"], json!([]));
}

#[test]
fn the_tool_text_editing_and_viewport_are_told_when_they_change() {
    let (mut u, _) = setup();
    u.app.run("tool.set", json!({"tool": "frame"}));
    u.frames(2, Vec::new);
    assert_eq!(heard_all(&mut u), [("tool.changed".to_string(), json!({"tool": "frame"}))]);

    let t = id(&u.app.session.execute("node.create", &json!({"type": "text", "x": 0, "y": 100, "text": "Hi"})).unwrap());
    u.frames(1, Vec::new);
    heard_all(&mut u);
    u.app.run("text.edit.begin", json!({"id": t}));
    u.frames(2, Vec::new);
    let uid = u.app.session.ds().unwrap().uid;
    let ev = heard_all(&mut u);
    assert!(ev.contains(&("text.editing".to_string(), json!({"document": uid, "id": t.0}))), "{ev:?}");
    u.app.run("text.edit.end", json!({}));
    u.frames(2, Vec::new);
    let ev = heard_all(&mut u);
    assert!(ev.contains(&("text.editing".to_string(), json!({"document": uid, "id": null}))), "{ev:?}");

    u.app.run("view.zoomIn", json!({}));
    u.app.run("view.zoomIn", json!({}));
    u.frames(1, Vec::new);
    let ev = heard_all(&mut u);
    let views: Vec<&Value> = ev.iter().filter(|(n, _)| n == "viewport.changed").map(|(_, d)| d).collect();
    assert_eq!(views.len(), 1, "once a frame: {ev:?}");
    let v = &u.app.session.ds().unwrap().view;
    assert_eq!(views[0]["zoom"], json!(v.zoom));
    assert_eq!(views[0]["x"], json!(v.cx - v.vw / (2.0 * v.zoom)));
    assert_eq!(views[0]["y"], json!(v.cy - v.vh / (2.0 * v.zoom)));
}

#[test]
fn an_open_interaction_says_whether_the_host_began_it() {
    let (mut u, r) = setup();
    let uid = u.app.session.ds().unwrap().uid;
    let interactions = |u: &mut Ui| -> Vec<Value> { heard_all(u).into_iter().filter(|(n, _)| n == "interaction.changed").map(|(_, d)| d).collect() };
    u.app.run("edit.begin", json!({"label": "Scrub"}));
    u.frames(2, Vec::new);
    assert_eq!(interactions(&mut u), [json!({"document": uid, "active": true, "label": "Scrub", "host": true})]);
    u.app.run("edit.preview", json!({"commands": [{"command": "node.set", "params": {"id": r, "x": 5}}]}));
    u.frames(2, Vec::new);
    assert_eq!(interactions(&mut u), Vec::<Value>::new(), "the same gesture");
    u.app.run("edit.commit", json!({}));
    u.frames(2, Vec::new);
    assert_eq!(interactions(&mut u), [json!({"document": uid, "active": false, "label": null, "host": false})]);
    // The canvas's own drag holds the same slot.
    u.app.session.begin_interaction("Move").unwrap();
    u.frames(2, Vec::new);
    assert_eq!(interactions(&mut u), [json!({"document": uid, "active": true, "label": "Move", "host": false})]);
}

#[test]
fn an_agents_edit_is_labelled_with_its_name() {
    let (mut u, r) = setup();
    u.app.session.execute_as(Some("Claude"), "node.set", &json!({"id": r, "x": 7})).unwrap();
    u.frames(2, Vec::new);
    assert_eq!(doc_change(&mut u)["agent"], json!("Claude (agent)"));
    u.app.run("node.set", json!({"id": r, "x": 8}));
    u.frames(2, Vec::new);
    assert_eq!(doc_change(&mut u)["agent"], Value::Null, "the person's edit");
}
