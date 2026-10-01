//! A render handle over `TestKernel` and the fake engine: two pages (object
//! 3, 2 by 1 points, and object 4, 4 by 2), and a `page.render.image` that
//! answers raw pixels the way `epdf-viewer-host` decodes the native host's
//! raster: the size, `color` and `premultipliedAlpha` as `PageRaster` spells
//! them, `source: {kind: "pixels", $byteLength}`, and a `$host` id naming the
//! pixels in a table the handle's pixel source reads.

#![allow(dead_code)]

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use epdf_ffi::{CancelToken, KernelThread, ViewerError};
use epdf_kernel::{Error, KernelOptions};
use epdf_plugin_render::ffi::RenderHandle;
use epdf_plugin_render::{RENDER, RenderConfig, RenderHost, render_plugin_with_host};
use futures::executor::block_on;
use plugin_conformance::test_kernel::{DOCUMENT, Reply, TestKernel, TestSpec};
use serde_json::{Value, json};

pub const RED: [u8; 4] = [255, 0, 0, 255];
pub const BLUE: [u8; 4] = [0, 0, 255, 255];

fn page(index: u32, pon: u32, width: u32, height: u32) -> Value {
    json!({
        "index": index,
        "ref": { "kind": "objectNumber", "pageObjectNumber": pon },
        "label": null,
        "size": { "width": width, "height": height },
        "rotation": 0,
        "userUnit": 1,
        "boxes": {
            "media": { "left": 0, "bottom": 0, "right": width, "top": height },
            "crop": { "left": 0, "bottom": 0, "right": width, "top": height }
        }
    })
}

/// How `page.render.image` answers.
#[derive(Clone)]
pub enum Engine {
    /// Raw pixels at the requested width, as the host's raster decodes.
    Pixels,
    /// This value, from the fake's table.
    Value(Value),
    /// Rejected with this error spec, from the fake's table.
    Error(Value),
    /// Pending under the label `raster` until the test settles it.
    Deferred,
}

fn engine_spec(responses: Value) -> Value {
    json!({
        "documents": {
            DOCUMENT: {
                "id": DOCUMENT,
                "security": { "allows": "*" },
                "pages": [page(0, 3, 2, 1), page(1, 4, 4, 2)],
                "responses": responses
            }
        }
    })
}

type Table = Rc<RefCell<HashMap<u64, Arc<Vec<u8>>>>>;

/// A `width` by `height` raster, `stride` bytes a row, red and blue in a
/// checkerboard (red at the origin).
pub fn pixels(width: u32, height: u32, stride: u32) -> Vec<u8> {
    let mut bytes = vec![0u8; (stride * height) as usize];
    for y in 0..height {
        for x in 0..width {
            let at = (y * stride + x * 4) as usize;
            let color = if (x + y) % 2 == 0 { RED } else { BLUE };
            bytes[at..at + 4].copy_from_slice(&color);
        }
    }
    bytes
}

fn pixel_spec(table: Table, requests: Arc<Mutex<Vec<(u32, Value)>>>) -> TestSpec {
    let next = Rc::new(Cell::new(0u64));
    TestSpec::new(engine_spec(json!([]))).respond_page("page.render.image", move |pon, args| {
        let options = args.first().cloned().unwrap_or(Value::Null);
        requests.lock().unwrap().push((pon, options.clone()));
        let (page_width, page_height) = if pon == 3 { (2.0, 1.0) } else { (4.0, 2.0) };
        let viewport = &options["viewport"];
        let width = match viewport["kind"].as_str() {
            Some("width") => viewport["width"].as_f64().unwrap_or(0.0),
            _ => viewport["scale"].as_f64().unwrap_or(1.0) * page_width,
        };
        if !(width.is_finite() && width > 0.0) {
            return Err(Error::engine("InvalidArg", "render viewport width must be positive"));
        }
        let width = width.round().max(1.0) as u32;
        let height = (f64::from(width) * page_height / page_width).round().max(1.0) as u32;
        let stride = width * 4;
        let bytes = pixels(width, height, stride);
        next.set(next.get() + 1);
        let id = next.get();
        let length = bytes.len();
        table.borrow_mut().insert(id, Arc::new(bytes));
        Ok(Reply::value(json!({
            "width": width,
            "height": height,
            "stride": stride,
            "color": "rgba8",
            "premultipliedAlpha": false,
            "source": { "kind": "pixels", "$byteLength": length },
            "$host": id
        })))
    })
}

pub struct Rig {
    pub thread: KernelThread<TestKernel>,
    pub render: Arc<RenderHandle>,
    requests: Arc<Mutex<Vec<(u32, Value)>>>,
    released: Arc<Mutex<Vec<u64>>>,
}

impl Rig {
    pub fn new(engine: Engine) -> Rig {
        Rig::build(engine, false)
    }

    pub fn pixels() -> Rig {
        Rig::new(Engine::Pixels)
    }

    /// With `release_closed_sessions`, as `epdf-viewer-host` runs the kernel:
    /// a closed document's capability goes once nothing outside holds it.
    pub fn releasing_closed_sessions() -> Rig {
        Rig::build(Engine::Pixels, true)
    }

    fn build(engine: Engine, release_closed_sessions: bool) -> Rig {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let released = Arc::new(Mutex::new(Vec::new()));
        let (seen, freed) = (requests.clone(), released.clone());
        let (render_tx, render_rx) = std::sync::mpsc::channel();
        let thread = KernelThread::spawn(move |post| {
            let table: Table = Rc::default();
            let spec = match engine {
                Engine::Pixels => pixel_spec(table.clone(), seen),
                Engine::Value(value) => TestSpec::new(engine_spec(
                    json!([{ "path": "page.render.image", "value": value }]),
                )),
                Engine::Error(error) => TestSpec::new(engine_spec(
                    json!([{ "path": "page.render.image", "error": error }]),
                )),
                Engine::Deferred => TestSpec::new(engine_spec(
                    json!([{ "path": "page.render.image", "defer": "raster" }]),
                )),
            };
            let releasing = table.clone();
            let host = RenderHost {
                release_image: Some(Rc::new(move |image: &Value| {
                    if let Some(id) = image.get("$host").and_then(Value::as_u64) {
                        freed.lock().unwrap().push(id);
                        releasing.borrow_mut().remove(&id);
                    }
                })),
                ..RenderHost::default()
            };
            let options = KernelOptions { release_closed_sessions, ..KernelOptions::default() };
            let kernel = TestKernel::open_with(
                vec![render_plugin_with_host(RenderConfig::default(), host)],
                spec,
                options,
            )?;
            let source = table.clone();
            let handle = RenderHandle::new(
                kernel.capability(&RENDER)?,
                post,
                Rc::new(move |id| source.borrow().get(&id).cloned()),
            );
            let _ = render_tx.send(handle);
            Ok(kernel)
        })
        .unwrap();
        let render = render_rx.recv().unwrap();
        Rig { thread, render, requests, released }
    }

    /// Wait for a job on the kernel thread: everything posted before it has
    /// run.
    pub fn ping(&self) {
        block_on(self.thread.call(|_| ())).unwrap();
    }

    pub fn close(&self) {
        block_on(self.thread.call(|k| k.close())).unwrap().unwrap();
    }

    pub fn emit(&self, event: Value) {
        block_on(self.thread.call(move |k| k.emit(&event))).unwrap().unwrap();
    }

    /// What the fake engine logged: calls, and `{"abort": index}` entries.
    pub fn engine_log(&self) -> Vec<Value> {
        block_on(self.thread.call(|k| k.engine.log_since(0))).unwrap()
    }

    pub fn aborts(&self) -> usize {
        self.engine_log().iter().filter(|entry| entry.get("abort").is_some()).count()
    }

    /// Every `page.render.image` the pixel responder answered: the page and
    /// the options.
    pub fn requests(&self) -> Vec<(u32, Value)> {
        self.requests.lock().unwrap().clone()
    }

    /// The `$host` ids `RenderHost::release_image` let go of.
    pub fn released(&self) -> Vec<u64> {
        self.released.lock().unwrap().clone()
    }

    /// The live list behind [`released`](Self::released), for a test that
    /// takes the rig apart.
    pub fn released_log(&self) -> Arc<Mutex<Vec<u64>>> {
        self.released.clone()
    }
}

pub fn code(error: &ViewerError) -> Option<&str> {
    match error {
        ViewerError::Kernel { code, .. } => code.as_deref(),
        ViewerError::Closed => None,
    }
}

pub fn token() -> Arc<CancelToken> {
    CancelToken::new()
}

/// Poll an export's future once, so its job reaches the kernel thread: an
/// `async fn` posts nothing until it is first polled. It must still be
/// pending.
pub fn start<F: Future>(future: &mut Pin<Box<F>>) {
    let waker = futures::task::noop_waker();
    let poll = future.as_mut().poll(&mut Context::from_waker(&waker));
    assert!(matches!(poll, Poll::Pending), "the call answered before the engine did");
}
