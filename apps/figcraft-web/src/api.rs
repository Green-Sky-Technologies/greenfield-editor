//! `window.figcraft`: the page command API (`craft-web-api`, storytold/craft-libs#413) plus FigCraft's
//! own file functions.
//!
//! The crate installs `request`, `execute`, `commands`, `docInspect`, `uiInspect` and `screenshot`
//! over the same JSON methods as the desktop control server (`docs/control-protocol.md`), as
//! promises with a timeout, so an agent driving FigCraft in a browser tab (or an `<iframe>` in
//! another app) uses the same commands as the CLI and the MCP server:
//!
//! ```js
//! await figcraft.execute("node.create", {type: "frame", name: "Home"})  // engine command → its result
//! await figcraft.request("document.inspect")                            // control method → its result
//! await figcraft.commands()                                             // command list
//! const {data} = await figcraft.screenshot()                            // the canvas as PNG (base64)
//! ```
//!
//! FigCraft adds, on the same object:
//!
//! ```js
//! await figcraft.openFile("home.fig", bytes)                            // `file.open` with the bytes
//! figcraft.captureDownloads(true)                                       // saves stay in the page…
//! figcraft.files(); await figcraft.readFile("home.figcraft")            // …to be read back
//! await figcraft.inspect()                                              // `document.inspect`
//! await figcraft.executeAs("Claude", "command.batch", {commands: []})   // `execute` for an AI agent
//! ```
//!
//! `executeAs(agent, command, params)` is `engine.execute` with the `agent` name set, as an MCP
//! client's calls are: the agent's comments are signed with it, and its avatar and outline show where
//! it works (`execute` itself has no agent). `command.batch` carries it to every step. A host that
//! knows when the agent's turn starts and ends holds the presence meanwhile with `agent.presence`.
//!
//! Results resolve with the method's `result` as soon as the frame that ran it ends; errors reject
//! with an `Error`.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, TryRecvError};

use base64::Engine as _;
use craft_web_api::methods::Methods;
use craft_web_api::page::{PageApi, Request};
use figcraft_ui_egui::{ControlReply, ControlRequest, RequestGate};
use serde_json::{Value, json};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{JsFuture, future_to_promise};

use crate::saved_files::SavedFiles;

/// The app id in `window.<app>`.
const APP: &str = "figcraft";

/// A waiting call looks for the app's reply when a frame ends ([`wake`]), and at the latest this
/// often, in milliseconds, for a reply sent between frames.
const FALLBACK_MS: i32 = 16;

thread_local! {
    static QUEUE: RefCell<Vec<ControlRequest>> = const { RefCell::new(Vec::new()) };
    static NEXT_ID: Cell<u64> = const { Cell::new(0) };
    static CTX: RefCell<Option<egui::Context>> = const { RefCell::new(None) };
    static CAPTURE: Cell<bool> = const { Cell::new(false) };
    static SAVED: RefCell<SavedFiles> = RefCell::new(SavedFiles::default());
    /// The waiting calls' resolvers, called when a frame ends.
    static WAKE: RefCell<Vec<js_sys::Function>> = const { RefCell::new(Vec::new()) };
}

pub fn connect(ctx: egui::Context) {
    CTX.with(|c| *c.borrow_mut() = Some(ctx));
}

/// The page's requests since the last frame, for the app's control queue.
pub fn take_requests() -> Vec<ControlRequest> {
    QUEUE.with(|q| std::mem::take(&mut *q.borrow_mut()))
}

/// Keep a save instead of downloading it, when the page asked for that. True when kept.
pub fn capture(name: &str, bytes: &[u8]) -> bool {
    if !CAPTURE.with(Cell::get) {
        return false;
    }
    SAVED.with(|s| s.borrow_mut().keep(name, bytes));
    true
}

fn to_js(v: &Value) -> JsValue {
    js_sys::JSON::parse(&v.to_string()).unwrap_or(JsValue::NULL)
}

/// A queued control request whose reply has not arrived. Dropping it before the reply (the page's
/// timeout fired) cancels the request if the app has not started it: the page was told it failed,
/// so the app must not run it late.
struct Waiting {
    reply: Receiver<ControlReply>,
    gate: RequestGate,
    answered: bool,
}

impl Waiting {
    /// The reply as the protocol's object once the app has answered or gone away; `None` while the
    /// request is still waiting.
    fn poll(&mut self) -> Option<Value> {
        match self.reply.try_recv() {
            Ok(ControlReply { result, .. }) => {
                self.answered = true;
                Some(match result {
                    Ok(value) => json!({"ok": true, "result": value}),
                    Err(error) => json!({"ok": false, "error": error}),
                })
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.answered = true;
                Some(json!({"ok": false, "error": "request dropped"}))
            }
        }
    }
}

impl Drop for Waiting {
    fn drop(&mut self) {
        if !self.answered {
            self.gate.cancel_if_open();
        }
    }
}

/// Queue a control request for the next frame and wait for the app's reply, as the protocol's
/// `{ok, result}` / `{ok: false, error}` object.
fn send(method: &str, params: Value) -> Pin<Box<dyn Future<Output = Value>>> {
    let (reply, rx) = std::sync::mpsc::channel();
    let id = NEXT_ID.with(|n| {
        n.set(n.get() + 1);
        n.get()
    });
    let gate = RequestGate::new();
    QUEUE.with(|q| q.borrow_mut().push(ControlRequest { id: json!(id), method: method.to_string(), params, reply, state: gate.clone() }));
    CTX.with(|c| {
        if let Some(ctx) = c.borrow().as_ref() {
            ctx.request_repaint();
        }
    });
    let mut waiting = Waiting { reply: rx, gate, answered: false };
    Box::pin(async move {
        loop {
            if let Some(reply) = waiting.poll() {
                return reply;
            }
            next_wake().await;
        }
    })
}

/// Resolves when the next frame ends, or after [`FALLBACK_MS`].
async fn next_wake() {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        if let Some(window) = web_sys::window() {
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, FALLBACK_MS);
        }
        WAKE.with(|w| w.borrow_mut().push(resolve));
    });
    let _ = JsFuture::from(promise).await;
}

/// A frame ended: the calls it answered resolve now, in the same task, before the browser paints
/// (not on the next poll, which cost every call up to a frame).
pub fn wake() {
    for resolve in WAKE.with(|w| std::mem::take(&mut *w.borrow_mut())) {
        let _ = resolve.call0(&JsValue::NULL);
    }
}

/// A reply as a promise outcome: the `result`, or an `Error` carrying the `error`.
fn outcome(reply: &Value) -> Result<JsValue, JsValue> {
    if reply.get("ok").and_then(Value::as_bool) == Some(true) {
        Ok(to_js(reply.get("result").unwrap_or(&Value::Null)))
    } else {
        Err(js_sys::Error::new(reply.get("error").and_then(Value::as_str).unwrap_or("error")).into())
    }
}

fn set_fn(obj: &js_sys::Object, name: &str, f: JsValue) -> Result<(), JsValue> {
    js_sys::Reflect::set(obj, &name.into(), &f).map(|_| ())
}

/// Install `window.figcraft`: the crate's functions, then FigCraft's own.
pub fn install() -> Result<(), JsValue> {
    let request: Request = Rc::new(|method, params| send(&method, params));
    Rc::new(PageApi::new(APP, Methods::default(), request)).install()?;
    let window = web_sys::window().ok_or("no window")?;
    let obj: js_sys::Object = js_sys::Reflect::get(&window, &APP.into())?.unchecked_into();
    set_fn(
        &obj,
        "inspect",
        Closure::<dyn Fn() -> js_sys::Promise>::new(|| future_to_promise(async { outcome(&send("document.inspect", json!({})).await) }))
            .into_js_value(),
    )?;
    set_fn(
        &obj,
        "executeAs",
        Closure::<dyn Fn(String, String, JsValue) -> js_sys::Promise>::new(|agent: String, command: String, params: JsValue| {
            let params = js_sys::JSON::stringify(&params).ok().and_then(|s| s.as_string()).and_then(|s| serde_json::from_str(&s).ok());
            let params = params.unwrap_or_else(|| json!({}));
            future_to_promise(async move { outcome(&send("engine.execute", json!({"command": command, "params": params, "agent": agent})).await) })
        })
        .into_js_value(),
    )?;
    set_fn(
        &obj,
        "openFile",
        Closure::<dyn Fn(String, js_sys::Uint8Array) -> js_sys::Promise>::new(|name: String, bytes: js_sys::Uint8Array| {
            let data = base64::engine::general_purpose::STANDARD.encode(bytes.to_vec());
            future_to_promise(async move {
                outcome(&send("engine.execute", json!({"command": "file.open", "params": {"name": name, "data": data}})).await)
            })
        })
        .into_js_value(),
    )?;
    set_fn(&obj, "captureDownloads", Closure::<dyn Fn(bool)>::new(|on: bool| CAPTURE.with(|c| c.set(on))).into_js_value())?;
    set_fn(
        &obj,
        "files",
        Closure::<dyn Fn() -> JsValue>::new(|| {
            SAVED.with(|s| to_js(&Value::Array(s.borrow().list().map(|(n, size)| json!({"path": n, "size": size})).collect())))
        })
        .into_js_value(),
    )?;
    set_fn(
        &obj,
        "readFile",
        Closure::<dyn Fn(String) -> Result<JsValue, JsValue>>::new(|name: String| {
            SAVED.with(|s| {
                s.borrow()
                    .get(&name)
                    .map(|b| js_sys::Uint8Array::from(b).into())
                    .ok_or_else(|| js_sys::Error::new(&format!("{name}: not found")).into())
            })
        })
        .into_js_value(),
    )?;
    Ok(())
}
