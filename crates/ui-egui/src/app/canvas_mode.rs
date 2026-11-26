//! Canvas mode: the editor draws only the canvas and its overlays, and the page around it draws
//! everything else (toolbar, layers, inspector, menus).
//!
//! On: no navigation bar, sidebars, tab bar, toolbar, home screen or menus. The keys keep working.
//! What would have opened a menu, popover or dialog instead becomes a request to the host: a
//! [`HostEvent`] named `ui.request` that says what was asked for and where on screen, in points
//! from the canvas's top-left corner (CSS pixels in the browser). Off brings the editor's own
//! panels back at once, with nothing reloaded.
//!
//! [`HostEvent`]s are also how the app tells the host what happened (see [`FigApp::host_event`]);
//! the browser shell drains them every frame ([`FigApp::take_host_events`]).

use super::{FigApp, NodeId, RightTab, Value, json};

/// The most events kept for a host that doesn't drain them (the desktop app never does): the
/// oldest go first.
const MAX_QUEUED: usize = 1024;

/// A message for the host page: `name` and its JSON `data`.
#[derive(Clone, Debug, PartialEq)]
pub struct HostEvent {
    pub name: String,
    pub data: Value,
}

/// The UI requests a host answers in canvas mode (the editor has no panel, tab or dialog to open
/// them in).
fn for_host(kind: &str) -> bool {
    ["dialog.", "panel.", "palette.", "home", "present", "preview"].iter().any(|p| kind.starts_with(p))
        || matches!(kind, "comment.new" | "contextMenu" | "layers.rename" | "layers.collapseAll" | "view.devCompare")
}

/// The tab a panel request leaves the editor on. In canvas mode the host draws the tabs, but the
/// canvas still follows them: it draws prototype links only under the Prototype tab.
fn tab_after(kind: &str, now: RightTab) -> Option<RightTab> {
    match kind {
        "panel.design" => Some(RightTab::Design),
        "panel.prototype" => Some(RightTab::Prototype),
        "panel.designPrototype" => Some(if now == RightTab::Prototype { RightTab::Design } else { RightTab::Prototype }),
        _ => None,
    }
}

impl FigApp {
    /// Queue an event for the host page.
    pub fn host_event(&mut self, name: &str, data: Value) {
        let queue = &mut self.ui.host_events;
        if queue.len() >= MAX_QUEUED {
            queue.remove(0);
        }
        queue.push(HostEvent { name: name.to_string(), data });
    }

    /// The events queued since the last call.
    pub fn take_host_events(&mut self) -> Vec<HostEvent> {
        std::mem::take(&mut self.ui.host_events)
    }

    /// Ask the host for `kind` (`contextMenu`, `colorPicker`, `comment.new`, `dialog.about`…) in
    /// place of opening it. `at` is where on screen it was asked for.
    pub(crate) fn host_request(&mut self, kind: &str, at: Option<egui::Pos2>, data: Value) {
        let screen = at.map(|p| json!({"x": p.x, "y": p.y}));
        self.host_event("ui.request", json!({"kind": kind, "screen": screen, "data": data}));
    }

    /// Turn canvas mode on or off. Turning it on closes what only the editor's own chrome shows.
    pub fn set_canvas_mode(&mut self, on: bool) {
        if self.ui.canvas_mode == on {
            return;
        }
        self.ui.canvas_mode = on;
        if on {
            crate::colorpicker::close(self);
            self.ui.context_menu = None;
            self.ui.key_menu = false;
            self.ui.palette = None;
            self.ui.main_menu = false;
        }
        self.host_event("canvas.mode", json!({"on": on}));
    }

    /// In canvas mode, hand the UI request `kind` to the host. False in the editor's own mode, or
    /// for a request the canvas answers itself.
    pub(crate) fn divert_ui_request(&mut self, kind: &str, data: &Value) -> bool {
        if !self.ui.canvas_mode || !for_host(kind) {
            return false;
        }
        if let Some(tab) = tab_after(kind, self.ui.right_tab) {
            self.ui.right_tab = tab;
        }
        // Where the click was: the Comment tool's request carries a page point.
        let at = match (kind, data.get("x").and_then(Value::as_f64), data.get("y").and_then(Value::as_f64)) {
            ("comment.new", Some(x), Some(y)) => Some(self.to_screen(x, y)),
            _ => None,
        };
        self.host_request(kind, at, data.clone());
        true
    }

    /// The right-click menu in canvas mode: the host draws it.
    pub(crate) fn host_context_menu(&mut self, at: egui::Pos2, ids: &[NodeId]) {
        self.host_request("contextMenu", Some(at), json!({"ids": ids.iter().map(|i| i.0).collect::<Vec<_>>()}));
    }
}
