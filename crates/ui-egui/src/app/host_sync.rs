//! What the host hears of the editor as it changes, so a host's own layers list, inspector and tool
//! row follow the canvas from the events alone, without reading anything back.
//!
//! Each goes out from the frame that sees the change (once a frame at most, however many commands
//! ran in it):
//!
//! - `selection.changed {document, page, ids, nodes}`: the layers selected now, on the page in front,
//!   and each one's fields as `node.get` gives them at depth 0 (`childCount`, not `children`).
//! - `document.changed {document, revision, undoDepth, agent, resync, nodes, added, removed, moved}`:
//!   the layers or their properties changed (an edit, undo, redo, or a gesture's preview, so a scrub
//!   sends one per frame). Panning and zooming don't; `revision` is `document.inspect`'s, which does
//!   move with them. `agent` names the AI agent (`"Claude"`) when one ran a command in the
//!   frame (`executeAs`, `engine.execute` with `agent`), else `null`.
//!   - `nodes`: `{id, ...fields}` for each layer added or changed: an added layer's every field, a
//!     changed one's fields that differ (`null` for a field it no longer has, such as `opacity` back
//!     at 1).
//!   - `added` and `moved`: `{id, parent, index}` where the layer is now; `removed`: where it was.
//!     `parent` is `null` for a page, whose `index` is its place among the pages. A host applies
//!     `removed`, takes `moved` out, then inserts `added` and `moved` in ascending `index`: the
//!     layers not listed keep their order.
//!   - `resync: true` (lists empty) when the host can't patch: the first frame of a file, another
//!     file in front, or more than [`MAX_PATCH`] layers changed at once. Read the tree again.
//! - `tool.changed {tool}`: the canvas tool's id (`tool.set`'s).
//! - `text.editing {document, id}`: the text layer being typed in, or `null`.
//! - `viewport.changed {document, zoom, x, y}`: the zoom (1 = 100%) and the page point at the canvas's
//!   top-left corner; a page point `p` shows at `(p - (x, y)) * zoom` in the canvas.
//! - `interaction.changed {document, active, label, host}`: an interaction (a drag, a resize, a
//!   gesture) is open on the document. `host` is true for one the host began with `edit.begin`; one
//!   with `host: false` is the canvas's own, and `edit.begin` fails until it ends.
//!
//! The first frame with a file (also another tab brought to the front) sends them all, so a host
//! starts from the state rather than from the next change.

use serde_json::Map;

use super::{Arc, FigApp, NodeId, Value, json};
use figcraft_doc::Document;
use figcraft_engine::inspect::node_json;

/// The most layers one `document.changed` patches; past it the event says `resync`.
pub const MAX_PATCH: usize = 1000;

/// What the host was last told.
#[derive(Clone, Default)]
pub struct Seen {
    file: Option<FileSeen>,
    tool: Option<&'static str>,
    agent_runs: u64,
}

/// What the host was last told of the file in front.
#[derive(Clone)]
struct FileSeen {
    document: u64,
    /// The file's layers as they were: a new `Arc` is an edit, whatever the revision counter says.
    doc: Arc<Document>,
    page: NodeId,
    selection: Vec<NodeId>,
    text: Option<NodeId>,
    viewport: [f64; 3],
    interaction: Option<(String, bool)>,
}

impl FigApp {
    /// Once a frame: tell the host what changed since the last frame.
    pub(crate) fn post_document_events(&mut self) {
        let before = std::mem::take(&mut self.ui.host_seen);
        let tool = self.session.tool.tool.id();
        if before.tool != Some(tool) {
            self.host_event("tool.changed", json!({"tool": tool}));
        }
        let now = self.session.ds().ok().map(|d| FileSeen {
            document: d.uid,
            doc: d.doc.clone(),
            page: d.page,
            selection: d.selection.clone(),
            text: d.text_edit.as_ref().map(|t| t.id),
            viewport: viewport(&d.view),
            interaction: d.interaction.as_ref().map(|it| (it.label.clone(), self.session.gesture_is_host())),
        });
        let agent_runs = self.session.agent_runs;
        self.ui.host_seen = Seen { file: now.clone(), tool: Some(tool), agent_runs };
        let same = match (&before.file, &now) {
            (None, None) => return,
            (Some(b), Some(n)) if b.document == n.document => Some(b),
            _ => None,
        };
        // `None` (tell everything) unless the same file was in front last frame.
        let pair = same.zip(now.as_ref());
        let uid = now.as_ref().map(|n| n.document);
        if pair.is_none_or(|(b, n)| b.selection != n.selection || b.page != n.page) {
            let (page, ids, nodes) = now.as_ref().map_or((None, Vec::new(), Vec::new()), |n| {
                (Some(n.page.0), n.selection.iter().map(|i| i.0).collect(), n.selection.iter().map(|i| node_json(&n.doc, *i, 0)).collect())
            });
            self.host_event("selection.changed", json!({"document": uid, "page": page, "ids": ids, "nodes": nodes}));
        }
        if pair.is_none_or(|(b, n)| !Arc::ptr_eq(&b.doc, &n.doc)) {
            let (revision, depth) = self.session.ds().map_or((0, 0), |d| (d.revision, self.session.undo_depth().0));
            let patch = pair.and_then(|(b, n)| Patch::between(&b.doc, &n.doc));
            let agent = (agent_runs != before.agent_runs).then(|| self.session.agent_presence.as_ref().map(|p| p.name.clone())).flatten();
            let resync = patch.is_none();
            let p = patch.unwrap_or_default();
            self.host_event(
                "document.changed",
                json!({"document": uid, "revision": revision, "undoDepth": depth, "agent": agent, "resync": resync,
                       "nodes": p.nodes, "added": p.added, "removed": p.removed, "moved": p.moved}),
            );
        }
        let Some(n) = &now else {
            if before.file.is_some() {
                self.host_event("text.editing", json!({"document": null, "id": null}));
                self.host_event("interaction.changed", json!({"document": null, "active": false, "label": null, "host": false}));
            }
            return;
        };
        if pair.is_none_or(|(b, _)| b.text != n.text) {
            self.host_event("text.editing", json!({"document": uid, "id": n.text.map(|i| i.0)}));
        }
        if pair.is_none_or(|(b, _)| b.viewport != n.viewport) {
            let [zoom, x, y] = n.viewport;
            self.host_event("viewport.changed", json!({"document": uid, "zoom": zoom, "x": x, "y": y}));
        }
        if pair.is_none_or(|(b, _)| b.interaction != n.interaction) {
            let (label, host) = n.interaction.clone().map_or((None, false), |(l, h)| (Some(l), h));
            self.host_event("interaction.changed", json!({"document": uid, "active": label.is_some(), "label": label, "host": host}));
        }
    }
}

/// Zoom and the page point at the canvas's top-left.
fn viewport(v: &figcraft_engine::ViewState) -> [f64; 3] {
    let zoom = if v.zoom > 0.0 { v.zoom } else { 1.0 };
    [v.zoom, v.cx - v.vw / (2.0 * zoom), v.cy - v.vh / (2.0 * zoom)]
}

/// The lists of a `document.changed` that patches.
#[derive(Default)]
struct Patch {
    nodes: Vec<Value>,
    added: Vec<Value>,
    removed: Vec<Value>,
    moved: Vec<Value>,
}

impl Patch {
    /// What changed from `before` to `now`, or `None` past [`MAX_PATCH`] layers.
    fn between(before: &Document, now: &Document) -> Option<Self> {
        let ids = now.changed_ids(before, MAX_PATCH)?;
        let mut p = Self::default();
        let mut moved: Vec<NodeId> = Vec::new();
        for id in ids {
            match (before.node(id), now.node(id)) {
                (None, Some(_)) => {
                    p.added.push(place(now, id));
                    p.nodes.push(node_json(now, id, 0));
                }
                (Some(_), None) => p.removed.push(place(before, id)),
                (Some(old), Some(new)) => {
                    if old.parent != new.parent {
                        moved.push(id);
                    }
                    if old.children != new.children {
                        moved.extend(reordered(&old.children, &new.children));
                    }
                    let fields = changed_fields(node_json(before, id, 0), node_json(now, id, 0));
                    if fields.len() > 1 {
                        p.nodes.push(Value::Object(fields));
                    }
                }
                (None, None) => {}
            }
        }
        if before.pages != now.pages {
            moved.extend(reordered(&before.pages, &now.pages));
        }
        moved.sort_unstable();
        moved.dedup();
        p.moved = moved.into_iter().filter(|id| before.contains(*id)).map(|id| place(now, id)).collect();
        Some(p)
    }
}

/// `{id, parent, index}`: where the layer is in `doc` (a page's parent is `null`).
fn place(doc: &Document, id: NodeId) -> Value {
    let parent = doc.node(id).and_then(|n| n.parent);
    let index = match parent {
        Some(p) => doc.node(p).and_then(|n| n.children.iter().position(|c| *c == id)),
        None => doc.pages.iter().position(|p| *p == id),
    };
    json!({"id": id.0, "parent": parent.map(|p| p.0), "index": index})
}

/// The id and the fields of `new` that differ from `old`; a field only `old` has is `null`.
fn changed_fields(old: Value, new: Value) -> Map<String, Value> {
    let (Value::Object(mut old), Value::Object(new)) = (old, new) else { return Map::new() };
    let mut out = Map::new();
    out.insert("id".into(), new.get("id").cloned().unwrap_or(Value::Null));
    for (k, v) in new {
        if old.remove(&k).as_ref() != Some(&v) {
            out.insert(k, v);
        }
    }
    for (k, _) in old {
        out.insert(k, Value::Null);
    }
    out
}

/// The children in both lists whose order among each other changed: the fewest that, taken out and
/// put back at their new places, turn `old` into `new` (those off a longest run kept in order).
fn reordered(old: &[NodeId], new: &[NodeId]) -> Vec<NodeId> {
    let was: std::collections::HashMap<NodeId, usize> = old.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    let kept: Vec<(NodeId, usize)> = new.iter().filter_map(|id| was.get(id).map(|i| (*id, *i))).collect();
    // Longest increasing run of old positions, in new order (patience sorting).
    let mut tails: Vec<usize> = Vec::new();
    let mut prev = vec![usize::MAX; kept.len()];
    for (k, (_, pos)) in kept.iter().enumerate() {
        let at = tails.partition_point(|&t| kept[t].1 < *pos);
        if at > 0 {
            prev[k] = tails[at - 1];
        }
        if at == tails.len() {
            tails.push(k);
        } else {
            tails[at] = k;
        }
    }
    let mut in_run = vec![false; kept.len()];
    let mut k = tails.last().copied().unwrap_or(usize::MAX);
    while k != usize::MAX {
        in_run[k] = true;
        k = prev[k];
    }
    kept.iter().zip(in_run).filter(|(_, run)| !run).map(|((id, _), _)| *id).collect()
}
