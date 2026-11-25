//! What the host hears of the file in front as it changes: `selection.changed` and
//! `document.changed`, so a host's own layers list and inspector follow the canvas without polling.
//!
//! Both go out from the frame that sees the change (once a frame at most, however many commands ran
//! in it) and say where things stand rather than what happened:
//!
//! - `selection.changed {document, page, ids}`: the layers selected now, on the page in front.
//! - `document.changed {document, revision, undoDepth}`: the layers or their properties changed (an
//!   edit, undo, redo, or a gesture's preview, so a scrub sends one per frame). Panning and zooming
//!   don't; `revision` is `document.inspect`'s, which does move with them.
//!
//! The first frame with a file (also another tab brought to the front) sends both, so a host starts
//! from the state rather than from the next change.

use super::{Arc, FigApp, NodeId, json};

/// What the host was last told of the file in front.
#[derive(Clone)]
pub struct Seen {
    document: u64,
    /// The file's layers as they were: a new `Arc` is an edit, whatever the revision counter says.
    doc: Arc<figcraft_doc::Document>,
    page: NodeId,
    selection: Vec<NodeId>,
}

impl FigApp {
    /// Once a frame: tell the host what changed in the file in front since the last frame.
    pub(crate) fn post_document_events(&mut self) {
        let now = self.session.ds().ok().map(|d| Seen { document: d.uid, doc: d.doc.clone(), page: d.page, selection: d.selection.clone() });
        let before = self.ui.host_seen.take();
        let (selection_moved, doc_moved) = match (&before, &now) {
            (None, None) => return,
            (Some(b), Some(n)) if b.document == n.document => (b.selection != n.selection || b.page != n.page, !Arc::ptr_eq(&b.doc, &n.doc)),
            _ => (true, true),
        };
        self.ui.host_seen = now.clone();
        if !selection_moved && !doc_moved {
            return;
        }
        let uid = now.as_ref().map(|n| n.document);
        if selection_moved {
            let (page, ids) = now.as_ref().map_or((None, Vec::new()), |n| (Some(n.page.0), n.selection.iter().map(|i| i.0).collect()));
            self.host_event("selection.changed", json!({"document": uid, "page": page, "ids": ids}));
        }
        if doc_moved {
            let (revision, depth) = self.session.ds().map_or((0, 0), |d| (d.revision, self.session.undo_depth().0));
            self.host_event("document.changed", json!({"document": uid, "revision": revision, "undoDepth": depth}));
        }
    }
}
