//! An attached AI agent's presence (#1416): its commands set it, on what they edited or selected;
//! it goes a short while after its last command; the user's commands never make one.

use std::time::Duration;

use figcraft_doc::NodeId;
use serde_json::{Value, json};

use crate::Session;
use crate::agent::LINGER;

fn id(v: &Value) -> NodeId {
    NodeId(v["id"].as_u64().unwrap())
}

fn rect(s: &mut Session, agent: Option<&str>, x: i32) -> NodeId {
    id(&s.execute_as(agent, "shape.rectangle", &json!({"x": x, "y": 0, "width": 40, "height": 40})).unwrap())
}

#[test]
fn an_agents_edit_shows_it_on_the_layers_it_changed() {
    let mut s = Session::new();
    let f = id(&s.execute("frame.create", &json!({"x": 0, "y": 0, "width": 300, "height": 200})).unwrap());
    let r = id(&s.execute_as(Some("Claude"), "shape.rectangle", &json!({"parent": f, "x": 10, "y": 10, "width": 40, "height": 40})).unwrap());
    let p = s.agent_presence.clone().expect("the agent is at work");
    assert_eq!(p.name, "Claude (agent)");
    assert_eq!(p.doc, s.ds().unwrap().uid);
    assert_eq!(p.layers, vec![r], "the new layer, not the frame whose children changed with it");
    assert_eq!(s.agent, None, "the command ran as the agent only");
}

#[test]
fn an_agents_selection_shows_it_there_and_a_look_keeps_it() {
    let mut s = Session::new();
    let (a, b) = (rect(&mut s, None, 0), rect(&mut s, None, 100));
    s.execute_as(Some(""), "select.set", &json!({"ids": [a, b]})).unwrap();
    let p = s.agent_presence.clone().unwrap();
    assert_eq!((p.name.as_str(), p.layers.clone()), ("AI agent", vec![a, b]));
    // A command that changes nothing keeps where it works and when it was last seen moves on.
    s.execute_as(Some(""), "view.zoomFit", &json!({})).unwrap();
    let again = s.agent_presence.clone().unwrap();
    assert_eq!(again.layers, vec![a, b]);
    assert!(again.at >= p.at);
    // So does a failed one.
    assert!(s.execute_as(Some(""), "node.set", &json!({"ids": [999_999], "props": {}})).is_err());
    assert_eq!(s.agent_presence.as_ref().unwrap().layers, vec![a, b]);
}

#[test]
fn a_held_presence_outlasts_the_linger_and_lingers_once_released() {
    let mut s = Session::new();
    let r = rect(&mut s, None, 0);
    assert_eq!(s.execute("agent.presence", &json!({"name": "Claude", "active": true})).unwrap(), json!({"name": "Claude (agent)", "active": true}));
    let held = s.agent_presence.clone().unwrap();
    assert!((held.name.as_str(), held.held, held.doc) == ("Claude (agent)", true, s.ds().unwrap().uid));
    let a_minute_on = held.at + Duration::from_secs(60);
    assert!(s.agent_presence_at(a_minute_on).is_some(), "a quiet agent still shows while the host holds it");

    // Its commands move where it works and keep the hold.
    let r2 = id(&s.execute_as(Some("Claude"), "shape.rectangle", &json!({"x": 80, "y": 0, "width": 10, "height": 10})).unwrap());
    let p = s.agent_presence.clone().unwrap();
    assert_eq!((p.held, p.layers), (true, vec![r2]));

    // Released: it shows for the usual linger from now.
    s.execute("agent.presence", &json!({"name": "Claude", "active": false})).unwrap();
    let p = s.agent_presence.clone().unwrap();
    assert!(!p.held);
    assert_eq!(p.layers, vec![r2], "where it worked stays for the linger");
    assert!(s.agent_presence_at(p.at + LINGER - Duration::from_millis(1)).is_some());
    assert!(s.agent_presence_at(p.at + LINGER).is_none());
    let _ = r;
}

#[test]
fn releasing_leaves_another_agents_presence_alone_and_needs_its_parameters() {
    let mut s = Session::new();
    rect(&mut s, Some("Codex"), 0);
    s.execute("agent.presence", &json!({"name": "Claude", "active": false})).unwrap();
    assert_eq!(s.agent_presence.as_ref().unwrap().name, "Codex (agent)", "Claude never had it");
    assert!(s.execute("agent.presence", &json!({"name": "Claude"})).is_err(), "active is required");
    assert!(s.execute("agent.presence", &json!({"active": true})).is_err(), "so is the name");
    s.execute("agent.presence", &json!({"name": "Claude", "active": true})).unwrap();
    assert_eq!(s.agent_presence.as_ref().unwrap().name, "Claude (agent)", "holding another agent replaces it");
}

#[test]
fn a_batch_run_as_an_agent_shows_the_agent_on_what_its_steps_edited() {
    let mut s = Session::new();
    let out = s
        .execute_as(
            Some("Claude"),
            "command.batch",
            &json!({"label": "Cards", "commands": [
                {"command": "shape.rectangle", "params": {"x": 0, "y": 0, "width": 40, "height": 40}, "as": "a"},
                {"command": "shape.rectangle", "params": {"x": 100, "y": 0, "width": 40, "height": 40}, "as": "b"},
            ]}),
        )
        .unwrap();
    let ids: Vec<NodeId> = out.as_array().unwrap().iter().map(id).collect();
    let p = s.agent_presence.clone().expect("the batch ran as the agent");
    assert_eq!(p.name, "Claude (agent)");
    assert_eq!(p.layers, ids, "both steps' layers");
    assert_eq!(s.agent, None);
}

#[test]
fn the_presence_goes_a_short_while_after_the_last_command() {
    let mut s = Session::new();
    rect(&mut s, Some("Claude"), 0);
    let at = s.agent_presence.as_ref().unwrap().at;
    assert!(s.agent_presence_at(at).is_some());
    assert!(s.agent_presence_at(at + LINGER - Duration::from_millis(1)).is_some());
    assert_eq!(s.agent_presence.as_ref().unwrap().left(at + Duration::from_secs(2)), LINGER - Duration::from_secs(2));
    assert!(s.agent_presence_at(at + LINGER).is_none(), "gone after the linger");
}

#[test]
fn the_users_commands_never_make_one() {
    let mut s = Session::new();
    let a = rect(&mut s, None, 0);
    s.execute("select.set", &json!({"ids": [a]})).unwrap();
    s.execute("node.set", &json!({"ids": [a], "props": {"x": 50}})).unwrap();
    assert!(s.agent_presence.is_none());
    // An agent's name on one file: the user's later edits elsewhere don't move it.
    rect(&mut s, Some("Claude"), 200);
    let before = s.agent_presence.clone();
    s.execute("node.set", &json!({"ids": [a], "props": {"x": 80}})).unwrap();
    assert_eq!(s.agent_presence, before);
}

#[test]
fn another_file_starts_afresh() {
    let mut s = Session::new();
    let a = rect(&mut s, Some("Claude"), 0);
    assert_eq!(s.agent_presence.as_ref().unwrap().layers, vec![a]);
    s.execute("file.new", &json!({})).unwrap();
    s.execute_as(Some("Claude"), "view.zoomFit", &json!({})).unwrap();
    let p = s.agent_presence.clone().unwrap();
    assert_eq!(p.doc, s.ds().unwrap().uid);
    assert!(p.layers.is_empty(), "the old file's layers don't follow it");
}

#[test]
fn a_frame_the_agent_changed_keeps_its_outline_when_it_also_adds_to_it() {
    let mut s = Session::new();
    let f = id(&s.execute("frame.create", &json!({"x": 0, "y": 0, "width": 300, "height": 200})).unwrap());
    let out = s
        .execute_as(
            Some("Claude"),
            "command.batch",
            &json!({"commands": [
                {"command": "node.set", "params": {"ids": [f], "props": {"fill": "#0B1F4B"}}},
                {"command": "text.create", "params": {"parent": f, "text": "Hello", "x": 10, "y": 10}},
            ]}),
        )
        .unwrap();
    let t = id(&out[1]);
    assert_eq!(s.agent_presence.clone().unwrap().layers, vec![f, t], "the frame's fill changed: it is the agent's work too");
}

#[test]
fn a_held_turn_outlines_every_layer_its_commands_touched() {
    let mut s = Session::new();
    let f = id(&s.execute("frame.create", &json!({"x": 0, "y": 0, "width": 300, "height": 200})).unwrap());
    let other = rect(&mut s, None, 400);
    s.execute("agent.presence", &json!({"name": "Claude", "active": true})).unwrap();
    s.execute_as(Some("Claude"), "node.set", &json!({"ids": [f], "props": {"fill": "#0B1F4B"}})).unwrap();
    let t = id(&s.execute_as(Some("Claude"), "text.create", &json!({"parent": f, "text": "Hello", "x": 10, "y": 10})).unwrap());
    s.execute_as(Some("Claude"), "node.set", &json!({"ids": [other], "props": {"x": 500}})).unwrap();
    s.execute_as(Some("Claude"), "select.set", &json!({"ids": [other]})).unwrap();
    assert_eq!(s.agent_presence.clone().unwrap().layers, vec![f, t, other], "every layer of the turn, in the order it touched them");
    s.execute_as(Some("Claude"), "edit.delete", &json!({"ids": [t]})).unwrap();
    assert_eq!(s.agent_presence.clone().unwrap().layers, vec![f, other], "a deleted layer drops out");

    // The next turn starts afresh.
    s.execute("agent.presence", &json!({"name": "Claude", "active": false})).unwrap();
    s.execute("agent.presence", &json!({"name": "Claude", "active": true})).unwrap();
    assert!(s.agent_presence.clone().unwrap().layers.is_empty());
}

#[test]
fn an_agents_edits_leave_the_users_selection_alone() {
    let mut s = Session::new();
    let f = id(&s.execute("frame.create", &json!({"x": 0, "y": 0, "width": 300, "height": 200})).unwrap());
    s.execute("select.set", &json!({"ids": [f]})).unwrap();
    let r = id(&s.execute_as(Some("Claude"), "shape.rectangle", &json!({"parent": f, "x": 10, "y": 10, "width": 40, "height": 40})).unwrap());
    assert_eq!(s.selection(), vec![f], "a new layer doesn't take the user's selection");
    assert_eq!(s.agent_presence.as_ref().unwrap().layers, vec![r], "the agent still shows on what it made");
    s.execute_as(Some("Claude"), "edit.duplicate", &json!({"ids": [r]})).unwrap();
    assert_eq!(s.selection(), vec![f], "nor does a copy");
}

#[test]
fn an_agents_batch_steps_see_what_earlier_steps_selected() {
    let mut s = Session::new();
    let f = id(&s.execute("frame.create", &json!({"x": 0, "y": 0, "width": 300, "height": 200})).unwrap());
    s.execute("select.set", &json!({"ids": [f]})).unwrap();
    s.execute_as(
        Some("Claude"),
        "command.batch",
        &json!({"commands": [
            {"command": "shape.rectangle", "params": {"parent": f, "x": 0, "y": 0, "width": 40, "height": 40}},
            {"command": "edit.duplicate", "params": {}},
        ]}),
    )
    .unwrap();
    assert_eq!(s.doc().unwrap().node(f).unwrap().children.len(), 2, "the copy is of the rectangle the first step selected");
    assert_eq!(s.selection(), vec![f]);
}

#[test]
fn a_layer_the_agent_deletes_leaves_the_users_selection() {
    let mut s = Session::new();
    let (a, b) = (rect(&mut s, None, 0), rect(&mut s, None, 100));
    s.execute("select.set", &json!({"ids": [a, b]})).unwrap();
    s.execute_as(Some("Claude"), "edit.delete", &json!({"ids": [a]})).unwrap();
    assert_eq!(s.selection(), vec![b]);
}

#[test]
fn the_users_own_new_layer_is_selected() {
    let mut s = Session::new();
    let a = rect(&mut s, None, 0);
    assert_eq!(s.selection(), vec![a]);
}
