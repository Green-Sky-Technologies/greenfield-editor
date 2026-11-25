//! Where an AI agent attached over MCP works, for the app to show like a person in a live
//! session: its name and the layers it last edited or selected, until a short while after its last
//! command ([`Session::execute_as`]). Display only: it is no undo step, it is never saved, and it is
//! not presence a live session sends (the agent shows there only if it joins one itself).
//!
//! A host that knows when the agent's turn starts and ends holds the presence for the whole turn
//! with `agent.presence` instead of leaving it to [`LINGER`], which runs from the last command.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use figcraft_doc::{Document, NodeId};
use serde_json::{Value, json};
use web_time::Instant;

use crate::params::{boolean, req_string};
use crate::{CommandSpec, Session, changed_nodes, spec};

/// How long an agent's presence shows after its last command.
pub const LINGER: Duration = Duration::from_secs(8);
/// Most layers an agent's presence names.
pub const MAX_LAYERS: usize = 500;

/// An attached AI agent at work.
#[derive(Clone, Debug, PartialEq)]
pub struct AgentPresence {
    /// As its comments are signed ("Claude (agent)").
    pub name: String,
    /// The file it last worked in (`DocState::uid`).
    pub doc: u64,
    /// The layers its last edit changed (the innermost), or that it last selected.
    pub layers: Vec<NodeId>,
    /// When its last command ran, or the host last changed what it holds.
    pub at: Instant,
    /// The host holds it (`agent.presence {active: true}`): it shows however long the agent is
    /// quiet, until the host lets go; then it lingers as it would after a command.
    pub held: bool,
}

impl AgentPresence {
    /// How much longer it shows at `now` (zero: gone). A held presence always has the full linger
    /// left.
    pub fn left(&self, now: Instant) -> Duration {
        if self.held {
            return LINGER;
        }
        LINGER.saturating_sub(now.saturating_duration_since(self.at))
    }
}

/// The document and selection a command started from: (file, document, selection).
pub(crate) type Before = (u64, Arc<Document>, Vec<NodeId>);

/// The layers a command worked on, going from `before` to `after`: the innermost layers it edited
/// (a parent changes with its children's list), else the selection it changed to; `None` when it
/// did neither.
fn touched(before: &Document, after: &Document, sel_before: &[NodeId], sel_after: &[NodeId]) -> Option<Vec<NodeId>> {
    let changed: Vec<NodeId> = if std::ptr::eq(before, after) { Vec::new() } else { changed_nodes(after, before) };
    let edited: Vec<&figcraft_doc::Node> = changed.iter().filter_map(|i| after.node(*i)).filter(|n| !n.is_page()).collect();
    if !edited.is_empty() {
        let parents: HashSet<NodeId> = edited.iter().filter_map(|n| n.parent).collect();
        return Some(edited.iter().map(|n| n.id).filter(|i| !parents.contains(i)).take(MAX_LAYERS).collect());
    }
    (sel_before != sel_after).then(|| sel_after.iter().copied().take(MAX_LAYERS).collect())
}

impl Session {
    /// The document and selection before a command, for [`Session::note_agent`].
    pub(crate) fn agent_before(&self) -> Option<Before> {
        self.ds().ok().map(|d| (d.uid, d.doc.clone(), d.selection.clone()))
    }

    /// The running command is the agent's: it is at work, on what it just edited or selected
    /// (else on what it worked on before in the same file).
    pub(crate) fn note_agent(&mut self, before: Option<Before>) {
        let (Some(name), Ok(ds)) = (self.agent.clone(), self.ds()) else { return };
        let uid = ds.uid;
        let now = before.filter(|b| b.0 == uid).and_then(|(_, doc, sel)| touched(&doc, &ds.doc, &sel, &ds.selection));
        let kept = self.agent_presence.take().filter(|p| p.doc == uid && p.name == name);
        let held = kept.as_ref().is_some_and(|p| p.held);
        let layers = now.or(kept.map(|p| p.layers)).unwrap_or_default();
        self.agent_presence = Some(AgentPresence { name, doc: uid, layers, at: Instant::now(), held });
    }

    /// `agent.presence`: the host holds the presence of the agent called `name` in the file in front
    /// (`active`), or lets go of it (it then lingers from now, as after a command). Another agent's
    /// presence is not touched by a release.
    pub fn hold_agent(&mut self, name: &str, active: bool) -> crate::Result<Value> {
        let name = crate::cmd::comment::agent_author(name);
        let uid = self.ds()?.uid;
        let now = Instant::now();
        let (mine, other) = match self.agent_presence.take() {
            Some(p) if p.name == name => (Some(p), None),
            other => (None, other),
        };
        self.agent_presence = if active {
            let layers = mine.filter(|p| p.doc == uid).map(|p| p.layers).unwrap_or_default();
            Some(AgentPresence { name: name.clone(), doc: uid, layers, at: now, held: true })
        } else {
            mine.map(|p| AgentPresence { held: false, at: now, ..p }).or(other)
        };
        Ok(json!({"name": name, "active": active}))
    }

    /// The agent at work at `now`, if one ran a command less than [`LINGER`] before.
    pub fn agent_presence_at(&self, now: Instant) -> Option<&AgentPresence> {
        self.agent_presence.as_ref().filter(|p| !p.left(now).is_zero())
    }
}

/// The `agent.presence` command.
pub fn specs() -> Vec<CommandSpec> {
    vec![spec!(
        "agent.presence",
        "Hold an agent's presence",
        "{name (the agent's client name, as in engine.execute's `agent`), active (true: it shows in the file in front until you release it; false: it goes after the usual few seconds)}  an agent's outline and avatar for a whole turn, not only after its commands",
        false,
        |s, p| s.hold_agent(&req_string("agent.presence", p, "name")?, boolean(p, "active").ok_or_else(|| crate::bad("agent.presence", "`active` (true or false) required"))?)
    )]
}
