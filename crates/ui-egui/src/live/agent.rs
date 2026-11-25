//! An AI agent attached over MCP, shown like a person in a live session while it works: its avatar
//! in the people row and, on the canvas, an outline in its colour around the layers it last edited
//! or selected, with its name. Both go a short while after its last command
//! ([`figcraft_engine::agent`]), and only the file it works in shows them.

use egui::{Align2, Pos2, Rect, Sense, Shape, Stroke, Ui, pos2, vec2};
use figcraft_engine::agent::AgentPresence;
use web_time::Instant;

use crate::FigApp;
use crate::a11y::Tip;
use crate::theme::Tokens;

/// The agent at work in the file in front; asks for the frame where it goes.
pub(crate) fn shown<'a>(app: &'a FigApp, ctx: &egui::Context) -> Option<&'a AgentPresence> {
    let now = Instant::now();
    let p = app.session.agent_presence_at(now).filter(|p| app.session.ds().is_ok_and(|d| d.uid == p.doc))?;
    ctx.request_repaint_after(p.left(now));
    Some(p)
}

/// What the host is told of the agent (`agent.presence`): whether it shows, who, on which layers.
#[derive(Clone, Debug, PartialEq)]
pub struct Posted {
    pub name: String,
    pub layers: Vec<figcraft_doc::NodeId>,
    pub held: bool,
}

/// Tell the host when the agent's presence starts, moves to other layers or ends. Once a frame, so
/// the end is seen on the frame the linger runs out ([`shown`] asks for it).
pub(crate) fn post_changes(app: &mut FigApp, ctx: &egui::Context) {
    let now = web_time::Instant::now();
    let shown = app.session.agent_presence_at(now).filter(|p| app.session.ds().is_ok_and(|d| d.uid == p.doc)).map(|p| {
        ctx.request_repaint_after(p.left(now));
        Posted { name: p.name.clone(), layers: p.layers.clone(), held: p.held }
    });
    if shown == app.ui.agent_posted {
        return;
    }
    let ids = |p: &Posted| p.layers.iter().map(|i| i.0).collect::<Vec<_>>();
    let data = match (&shown, &app.ui.agent_posted) {
        (Some(p), _) => serde_json::json!({"active": true, "name": p.name, "held": p.held, "layers": ids(p)}),
        (None, Some(last)) => serde_json::json!({"active": false, "name": last.name, "held": false, "layers": []}),
        (None, None) => return,
    };
    app.ui.agent_posted = shown;
    app.host_event("agent.presence", data);
}

/// Its avatar at `c` in the people row: ringed in its colour while it works, its name as the tip.
pub(super) fn avatar(ui: &mut Ui, c: Pos2, r: f32, name: &str) {
    let t = Tokens::get(ui.ctx());
    ui.interact(Rect::from_center_size(c, vec2(2.0 * r, 2.0 * r)), ui.scope_id().with("live_agent"), Sense::hover()).tip(name);
    ui.painter().circle_stroke(c, r + 1.0, Stroke::new(2.0, t.panel));
    super::avatar(ui.painter(), c, r, name, t.agent);
    ui.painter().circle_stroke(c, r + 2.5, Stroke::new(2.0, t.agent));
}

/// Its layers on the page in front outlined in its colour, its name over them.
pub fn draw(app: &FigApp, painter: &egui::Painter, ctx: &egui::Context) {
    let Some(p) = shown(app, ctx) else { return };
    let ember = Tokens::get(ctx).agent;
    let Ok(ds) = app.session.ds() else { return };
    let (doc, page) = (&ds.doc, ds.page);
    let mut over: Option<Rect> = None;
    for q in p.layers.iter().filter(|n| doc.page_of(**n) == Some(page)).filter_map(|n| crate::canvas::node_quad(app, doc, *n)) {
        painter.add(Shape::closed_line(q.to_vec(), Stroke::new(1.5, ember)));
        let r = Rect::from_points(&q);
        over = Some(over.map_or(r, |o| o.union(r)));
    }
    if let Some(r) = over {
        super::name_tag(painter, pos2(r.min.x, r.min.y - 4.0), Align2::LEFT_BOTTOM, &p.name, ember);
    }
}

#[cfg(test)]
mod tests {
    use figcraft_engine::Session;
    use serde_json::json;

    use super::shown;
    use crate::present_tests::Ui;

    fn rect(u: &mut Ui, agent: Option<&str>) {
        let mut p = json!({"command": "shape.rectangle", "params": {"x": 0, "y": 0, "width": 40, "height": 40}});
        if let Some(a) = agent {
            p["agent"] = json!(a);
        }
        let ctx = u.ctx.clone();
        crate::control::handle(&mut u.app, &ctx, "engine.execute", &p).unwrap();
        u.frames(2, Vec::new);
    }

    fn labels(u: &mut Ui) -> Vec<(String, egui::Pos2)> {
        let out = u.frame_output(vec![]);
        let nodes = out.platform_output.accesskit_update.map(|t| t.nodes).unwrap_or_default();
        nodes
            .into_iter()
            .filter_map(|(_, n)| {
                let b = n.bounds()?;
                Some((n.label()?.to_string(), egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32)))
            })
            .collect()
    }

    fn presence_events(u: &mut Ui) -> Vec<serde_json::Value> {
        u.app.take_host_events().into_iter().filter(|e| e.name == "agent.presence").map(|e| e.data).collect()
    }

    #[test]
    fn the_host_hears_the_agent_arrive_move_and_leave() {
        let mut u = Ui::new(Session::new());
        assert!(presence_events(&mut u).is_empty(), "nobody yet");
        rect(&mut u, Some("Claude"));
        let first = presence_events(&mut u);
        assert_eq!(first.len(), 1, "{first:?}");
        assert_eq!((first[0]["active"].clone(), first[0]["name"].clone(), first[0]["held"].clone()), (json!(true), json!("Claude"), json!(false)));
        let layer = first[0]["layers"][0].as_u64().unwrap();
        u.frames(3, Vec::new);
        assert!(presence_events(&mut u).is_empty(), "no news is no event");

        // The agent edits another layer: its layers moved.
        rect(&mut u, Some("Claude"));
        let moved = presence_events(&mut u);
        assert_eq!(moved.len(), 1);
        assert_ne!(moved[0]["layers"][0].as_u64().unwrap(), layer);

        // Eight seconds on (the linger) the frame that sees it gone tells the host.
        let at = u.app.session.agent_presence.as_ref().unwrap().at;
        u.app.session.agent_presence.as_mut().unwrap().at = at - std::time::Duration::from_secs(9);
        u.frames(2, Vec::new);
        let gone = presence_events(&mut u);
        assert_eq!(gone, vec![json!({"active": false, "name": "Claude", "held": false, "layers": []})]);
    }

    #[test]
    fn a_held_presence_is_posted_as_held_and_stays_through_the_linger() {
        let mut u = Ui::new(Session::new());
        u.app.run("agent.presence", json!({"name": "Claude", "active": true}));
        u.frames(2, Vec::new);
        let on = presence_events(&mut u);
        assert_eq!(on.len(), 1);
        assert_eq!((on[0]["active"].clone(), on[0]["held"].clone()), (json!(true), json!(true)));
        let at = u.app.session.agent_presence.as_ref().unwrap().at;
        u.app.session.agent_presence.as_mut().unwrap().at = at - std::time::Duration::from_secs(600);
        u.frames(3, Vec::new);
        assert!(presence_events(&mut u).is_empty(), "ten quiet minutes, still there");
        assert!(shown(&u.app, &u.ctx).is_some());
        u.app.run("agent.presence", json!({"name": "Claude", "active": false}));
        u.frames(2, Vec::new);
        let off = presence_events(&mut u);
        assert_eq!(off.len(), 1, "released: held is over, it lingers");
        assert_eq!((off[0]["active"].clone(), off[0]["held"].clone()), (json!(true), json!(false)));
    }

    #[test]
    fn the_people_menu_still_lists_you_beside_an_agent() {
        let mut u = Ui::new(Session::new());
        u.ctx.enable_accesskit();
        rect(&mut u, Some("Claude"));
        let (_, at) = labels(&mut u).into_iter().find(|(l, _)| l == "People in this file").expect("the people row's menu");
        u.button(at, true);
        u.button(at, false);
        u.frames(2, Vec::new);
        let shown = labels(&mut u);
        assert!(shown.iter().any(|(l, _)| l.ends_with("(you)")), "{shown:?}");
    }

    #[test]
    fn an_attached_agent_shows_in_the_file_it_works_in() {
        let mut u = Ui::new(Session::new());
        rect(&mut u, None);
        assert!(shown(&u.app, &u.ctx).is_none(), "the control channel's own calls are the user's");
        rect(&mut u, Some("Claude"));
        let p = shown(&u.app, &u.ctx).expect("the agent shows");
        assert_eq!((p.name.as_str(), p.layers.len()), ("Claude", 1));
        u.run("file.new", json!({}));
        assert!(shown(&u.app, &u.ctx).is_none(), "another file in front shows no agent");
    }
}
