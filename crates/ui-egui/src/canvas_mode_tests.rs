//! Canvas mode: only the canvas and its overlays are drawn; right-clicks, popovers, dialogs and the
//! Comment tool's click go to the host; the keys keep working; turning it off brings the panels
//! back with nothing reloaded.

use std::sync::Arc;

use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use figcraft_engine::Session;
use figcraft_engine::tools::ToolId;
use serde_json::{Value, json};

use crate::HostEvent;
use crate::colorpicker::PickerTarget;
use crate::live_tests::{flat, has_text};
use crate::present_tests::{Ui, id};

const SCREEN: egui::Vec2 = egui::vec2(1440.0, 900.0);

fn doc() -> (Ui, figcraft_doc::NodeId) {
    let mut s = Session::new();
    let r = id(&s.execute("node.create", &json!({"type": "rectangle", "x": 100, "y": 100, "width": 200, "height": 120})).unwrap());
    s.execute("select.none", &json!({})).unwrap();
    let mut u = Ui::new(s);
    u.frame(vec![]);
    (u, r)
}

fn drawn(u: &mut Ui, text: &str) -> bool {
    let out = u.frame_output(vec![]);
    has_text(&flat(&out.shapes), text)
}

fn events(u: &mut Ui, name: &str) -> Vec<Value> {
    u.app.take_host_events().into_iter().filter(|e| e.name == name).map(|HostEvent { data, .. }| data).collect()
}

fn click(u: &mut Ui, at: Pos2, button: PointerButton) {
    let ev = |pressed| Event::PointerButton { pos: at, button, pressed, modifiers: Modifiers::NONE };
    u.frame(vec![Event::PointerMoved(at)]);
    u.frame(vec![ev(true)]);
    u.frame(vec![ev(false)]);
}

fn press(u: &mut Ui, key: Key) {
    u.frame(vec![
        Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE },
        Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE },
    ]);
}

#[test]
fn canvas_mode_draws_no_chrome_and_the_canvas_fills_the_window() {
    let (mut u, _) = doc();
    let normal = u.app.canvas.rect;
    assert!(normal.width() < 1440.0 - 100.0, "the editor's panels take room beside the canvas: {normal:?}");
    assert!(drawn(&mut u, "Design"), "the Design panel is drawn");

    u.app.run("view.canvasMode", json!({"on": true}));
    u.frames(2, Vec::new);
    assert!(u.app.ui.canvas_mode);
    assert_eq!(u.app.inspect()["canvasMode"], json!(true), "ui.inspect says so");
    assert_eq!(u.app.canvas.rect.size(), SCREEN, "the canvas takes the whole window: no rail, panels, tab bar or toolbar");
    for chrome in ["Design", "Prototype", "Layers", "Assets", "Untitled"] {
        assert!(!drawn(&mut u, chrome), "{chrome} is not drawn");
    }
    assert_eq!(events(&mut u, "canvas.mode"), vec![json!({"on": true})], "the host hears it");
}

#[test]
fn turning_it_off_brings_the_panels_back_without_a_reload() {
    let (mut u, r) = doc();
    let before = u.app.canvas.rect;
    u.app.run("view.canvasMode", json!({"on": true}));
    u.frames(2, Vec::new);
    // The document survives the round trip: same session, same layer.
    u.app.run("view.canvasMode", json!({"on": false}));
    u.frames(2, Vec::new);
    assert!(!u.app.ui.canvas_mode);
    assert_eq!(u.app.canvas.rect, before);
    assert!(drawn(&mut u, "Design"));
    assert!(u.app.session.doc().unwrap().node(r).is_some());
}

#[test]
fn the_command_without_on_toggles() {
    let (mut u, _) = doc();
    u.app.run("view.canvasMode", json!({}));
    assert!(u.app.ui.canvas_mode);
    u.app.run("view.canvasMode", json!({}));
    assert!(!u.app.ui.canvas_mode);
}

#[test]
fn a_right_click_asks_the_host_for_a_menu_with_the_position_and_target() {
    let (mut u, r) = doc();
    u.app.run("view.canvasMode", json!({"on": true}));
    u.frames(2, Vec::new);
    let at = u.app.to_screen(200.0, 160.0);
    click(&mut u, at, PointerButton::Secondary);
    assert!(u.app.ui.context_menu.is_none(), "the editor opens no menu of its own");
    let asks = events(&mut u, "ui.request");
    assert_eq!(asks.len(), 1, "{asks:?}");
    assert_eq!(asks[0]["kind"], "contextMenu");
    assert_eq!(asks[0]["screen"], json!({"x": at.x, "y": at.y}));
    assert_eq!(asks[0]["data"]["ids"], json!([r.0]), "the layer under the pointer was selected and is the target");
}

#[test]
fn outside_canvas_mode_a_right_click_still_opens_the_editors_menu() {
    let (mut u, _) = doc();
    let at = u.app.to_screen(200.0, 160.0);
    click(&mut u, at, PointerButton::Secondary);
    assert!(u.app.ui.context_menu.is_some());
    assert!(events(&mut u, "ui.request").is_empty());
}

#[test]
fn the_colour_picker_is_the_hosts_to_draw() {
    let (mut u, r) = doc();
    u.app.run("view.canvasMode", json!({"on": true}));
    let button = egui::Rect::from_min_size(Pos2::new(50.0, 60.0), egui::vec2(20.0, 20.0));
    u.app.open_picker(PickerTarget::Paint { fills: true, index: 0, ids: vec![r] }, Some(button));
    assert!(u.app.ui.picker.is_none(), "no picker of the editor's");
    let asks = events(&mut u, "ui.request");
    assert_eq!(asks.len(), 1);
    assert_eq!(asks[0]["kind"], "colorPicker");
    assert_eq!(asks[0]["screen"], json!({"x": 50.0, "y": 80.0}), "beside the control that asked");
    assert_eq!(asks[0]["data"], json!({"target": "paint", "slot": "fill", "index": 0, "ids": [r.0]}));
    // The same request again is asked again (the host decides whether that closes its picker).
    u.app.toggle_picker(PickerTarget::Paint { fills: true, index: 0, ids: vec![r] }, Some(button));
    assert_eq!(events(&mut u, "ui.request").len(), 1);
}

#[test]
fn the_comment_tool_s_click_goes_to_the_host_with_its_screen_position() {
    let (mut u, _) = doc();
    u.app.run("view.canvasMode", json!({"on": true}));
    u.app.run("tool.comment", json!({}));
    u.frames(2, Vec::new);
    let at = Pos2::new(640.0, 400.0);
    click(&mut u, at, PointerButton::Primary);
    assert!(u.app.ui.comments.draft.is_none(), "no composer of the editor's");
    let asks = events(&mut u, "ui.request");
    assert_eq!(asks.len(), 1, "{asks:?}");
    assert_eq!(asks[0]["kind"], "comment.new");
    let page = u.app.to_page(at);
    assert_eq!((asks[0]["data"]["x"].as_f64().unwrap(), asks[0]["data"]["y"].as_f64().unwrap()), (page.x, page.y));
    assert_eq!(asks[0]["screen"], json!({"x": at.x, "y": at.y}));
}

#[test]
fn dialogs_and_panels_the_commands_ask_for_go_to_the_host() {
    let (mut u, _) = doc();
    u.app.run("view.canvasMode", json!({"on": true}));
    u.app.run("help.about", json!({}));
    u.app.run("view.layers", json!({}));
    assert!(!u.app.ui.about, "no About window");
    let kinds: Vec<String> = events(&mut u, "ui.request").iter().map(|d| d["kind"].as_str().unwrap().to_string()).collect();
    assert_eq!(kinds, ["dialog.about", "panel.layers"]);
    // Outside canvas mode the same commands open the editor's own.
    u.app.run("view.canvasMode", json!({"on": false}));
    u.app.run("help.about", json!({}));
    assert!(u.app.ui.about);
}

#[test]
fn the_keys_keep_working_on_the_canvas() {
    let (mut u, _) = doc();
    u.app.run("view.canvasMode", json!({"on": true}));
    u.frames(2, Vec::new);
    press(&mut u, Key::R);
    assert_eq!(u.app.session.tool.tool, ToolId::Rectangle);
    press(&mut u, Key::V);
    assert_eq!(u.app.session.tool.tool, ToolId::Move);
}

#[test]
fn with_no_file_open_it_draws_an_empty_canvas_not_the_home_screen() {
    let mut u = Ui::new(Session::empty());
    assert!(drawn(&mut u, "Recents"), "the home screen is the editor's own start");
    u.app.run("view.canvasMode", json!({"on": true}));
    u.frames(2, Vec::new);
    for home in ["Recents", "Templates", "Online"] {
        assert!(!drawn(&mut u, home), "{home} is not drawn");
    }
    // A file the host opens shows at once.
    u.app.run("file.new", json!({}));
    u.frames(2, Vec::new);
    assert!(u.app.session.ds().is_ok());
    assert_eq!(u.app.canvas.rect.size(), SCREEN);
}

#[test]
fn a_host_that_never_drains_cannot_grow_the_queue_without_end() {
    let (mut u, _) = doc();
    for i in 0..5000 {
        u.app.host_event("test", json!({"i": i}));
    }
    let kept = u.app.take_host_events();
    assert_eq!(kept.len(), 1024);
    assert_eq!(kept.last().unwrap().data, json!({"i": 4999}), "the newest are kept");
}

fn backdrop(u: &Ui) -> crate::canvas::Backdrop {
    crate::canvas::backdrop(&u.app, &u.ctx, &crate::theme::Tokens::get(&u.ctx))
}

#[test]
fn in_canvas_mode_the_canvas_follows_the_theme_without_editing_the_file() {
    let (mut u, _) = doc();
    let page = |u: &Ui| u.app.session.ds().unwrap().doc.clone();
    let (before, depth) = (page(&u), u.app.session.undo_depth());
    let off = backdrop(&u);
    assert_eq!((off.background, off.dots), (None, None), "outside canvas mode the page's own background, no grid");

    u.app.run("view.canvasMode", json!({"on": true}));
    for (theme, tokens) in [("view.themeDark", crate::theme::Tokens::dark()), ("view.themeLight", crate::theme::Tokens::light())] {
        u.app.run(theme, json!({}));
        u.frames(2, Vec::new);
        let b = backdrop(&u);
        assert_eq!(b.bg, tokens.canvas, "{theme}");
        assert_eq!(b.background, Some(crate::theme::color_of(tokens.canvas)), "{theme}: the render's background too");
        let dots = b.dots.expect("the brand's dot grid");
        assert_eq!(dots.color.to_rgba8(), [tokens.text.r(), tokens.text.g(), tokens.text.b(), 23], "{theme}: ink at 9%");
        assert!(!dots.lines);
        assert_eq!(dots.step, 16.0, "every 16 px at 100%");
    }
    assert!(Arc::ptr_eq(&before, &page(&u)), "the file is not edited");
    assert_eq!(u.app.session.undo_depth(), depth, "no undo step");
}

#[test]
fn a_page_given_a_colour_keeps_it_in_canvas_mode() {
    let (mut u, _) = doc();
    u.app.run("view.canvasMode", json!({"on": true}));
    u.app.run("view.themeDark", json!({}));
    u.app.run("page.setBackground", json!({"color": "#336699"}));
    u.frames(2, Vec::new);
    let b = backdrop(&u);
    assert_eq!((b.background, b.dots), (None, None));
    assert_eq!(b.bg, egui::Color32::from_rgb(0x33, 0x66, 0x99));
}

fn links_drawn(u: &mut Ui) -> usize {
    let out = u.frame_output(vec![]);
    flat(&out.shapes).iter().filter(|s| matches!(s, egui::Shape::CubicBezier(_))).count()
}

#[test]
fn the_hosts_prototype_tab_shows_prototype_links_on_the_canvas() {
    let (mut u, r) = doc();
    let f = id(&u.app.session.execute("node.create", &json!({"type": "frame", "x": 600, "y": 0, "width": 200, "height": 300})).unwrap());
    u.app.session.execute("proto.interaction.add", &json!({"id": r, "dest": f})).unwrap();
    u.app.run("view.canvasMode", json!({"on": true}));
    u.frames(2, Vec::new);
    assert_eq!(links_drawn(&mut u), 0, "the Design tab draws no links");

    u.app.run("view.prototype", json!({}));
    assert_eq!(links_drawn(&mut u), 1, "the Prototype tab draws the link from the layer to its frame");
    let kinds: Vec<Value> = events(&mut u, "ui.request").iter().map(|d| d["kind"].clone()).collect();
    assert_eq!(kinds, [json!("panel.prototype")], "the host still hears it, to show its own tab");

    u.app.run("view.design", json!({}));
    assert_eq!(links_drawn(&mut u), 0);
    u.app.run("view.designPrototype", json!({}));
    assert_eq!(links_drawn(&mut u), 1, "Shift+E switches to it as well");
}
