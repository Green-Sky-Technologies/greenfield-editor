//! `document.inspect` reports the document's `revision` and undo depth, so a host can tell whether
//! anything changed without diffing the tree.

use serde_json::{Value, json};

use crate::Session;
use crate::inspect::document;

fn ex(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

fn revision(s: &Session) -> u64 {
    document(s, 0, false)["revision"].as_u64().expect("revision")
}

fn undo_depth(s: &Session) -> u64 {
    document(s, 0, false)["history"]["undoDepth"].as_u64().expect("undoDepth")
}

#[test]
fn revision_and_undo_depth_track_edits_undo_and_redo() {
    let mut s = Session::new();
    let (r0, d0) = (revision(&s), undo_depth(&s));
    assert_eq!(revision(&s), r0, "inspecting changes nothing");

    ex(&mut s, "shape.rectangle", json!({"x": 0, "y": 0, "width": 10, "height": 10}));
    let r1 = revision(&s);
    assert!(r1 > r0, "an edit bumps the revision");
    assert_eq!(undo_depth(&s), d0 + 1, "an edit is one undo step");

    ex(&mut s, "edit.undo", json!({}));
    let r2 = revision(&s);
    assert!(r2 > r1, "undo bumps the revision, it never goes back to an earlier one");
    assert_eq!(undo_depth(&s), d0);

    ex(&mut s, "edit.redo", json!({}));
    assert!(revision(&s) > r2, "redo bumps the revision");
    assert_eq!(undo_depth(&s), d0 + 1);
}
