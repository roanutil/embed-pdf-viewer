//! The typed API, beyond what the vectors reach.
//!
//! The vectors script handlers in JSON and give every page ref in a spelling
//! the kernel no longer reads (`{kind, pageObjectNumber}`), so they can't
//! show a tool's own pointer methods, a same-page `samplePointOn` fallback,
//! the `tools` setting, or what a Rust handler that leaves members out gets.
//! These tests do, against `packages/plugin/interaction/src/controller.ts`.

use std::cell::RefCell;
use std::rc::Rc;

use epdf_geometry::Point;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::kernel::OpenSource;
use epdf_kernel::{Kernel, KernelOptions, OpenOptions, rt};
use epdf_plugin_interaction::*;
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use serde_json::json;

fn page(index: u32, pon: u32) -> serde_json::Value {
    let box_ = json!({ "left": 0, "bottom": 0, "right": 600, "top": 800 });
    json!({
        "index": index, "ref": { "kind": "objectNumber", "objectNumber": pon }, "label": null,
        "size": { "width": 600, "height": 800 }, "rotation": 0, "userUnit": 1,
        "boxes": { "media": box_, "crop": box_ }
    })
}

fn kernel_with(config: InteractionConfig) -> Kernel {
    rt::reset();
    let doc = json!({ "pages": [page(0, 1), page(1, 2)] });
    let fake = FakeEngine::new(json!({ "documents": { "a": doc, "b": doc } }));
    let kernel = Kernel::new(
        Rc::new(AsEngine(fake)),
        vec![interaction_plugin(config), feedback_plugin(FeedbackPluginOptions::default())],
        KernelOptions { report: None, on_dispatch: None, release_closed_sessions: false },
    )
    .unwrap();
    let _ = kernel.start();
    rt::run_until_stalled();
    for id in ["a", "b"] {
        let _ = kernel
            .documents()
            .open(OpenSource::Input(json!({ "kind": "bytes", "id": id })), OpenOptions::default());
        rt::run_until_stalled();
    }
    kernel
}

fn kernel() -> Kernel {
    kernel_with(InteractionConfig::default())
}

fn hub(kernel: &Kernel, doc: &str) -> InteractionCapability {
    kernel.capability(&INTERACTION, Some(doc)).unwrap()
}

type Log = Rc<RefCell<Vec<String>>>;

/// A handler that implements only the required members and `on_up`.
struct UpOnly(Log);

impl InteractionHandler for UpOnly {
    fn id(&self) -> &str {
        "up-only"
    }
    fn priority(&self) -> f64 {
        1.0
    }
    fn enabled_for(&self, tool: &Tool) -> bool {
        tool.enables("text-select")
    }
    fn on_down(&self, _: &PointerSample) -> bool {
        self.0.borrow_mut().push("down".into());
        true
    }
    fn on_up(&self, _: &PointerSample) {
        self.0.borrow_mut().push("up".into());
    }
}

fn hit(pon: u32, x: f64, y: f64) -> PageHit {
    PageHit {
        page: PageRef::new(pon),
        point: Point { x, y },
        scale: None,
        rotation: None,
        zoom: None,
    }
}

fn sample(phase: Phase) -> PointerSample {
    PointerSample::new(phase, Point { x: 0.0, y: 0.0 }, Some(hit(1, 1.0, 1.0)))
}

fn on(phase: Phase, page: Option<PageHit>) -> PointerSample {
    PointerSample::new(phase, Point { x: 0.0, y: 0.0 }, page)
}

fn event_text(e: &ToolPointerEvent) -> String {
    format!("{}@{},{}", e.page.page_object_number, e.point.x, e.point.y)
}

/// A tool with all four pointer members, logging page-space events.
fn drawing_tool(id: &str, log: &Log, takes: bool) -> Tool {
    let mut tool = Tool::new(id, "crosshair", ["draw"]);
    let l = log.clone();
    tool.on_pointer_down = Some(Rc::new(move |e| {
        l.borrow_mut().push(format!("down {}", event_text(e)));
        takes
    }));
    let l = log.clone();
    tool.on_pointer_move =
        Some(Rc::new(move |e| l.borrow_mut().push(format!("move {}", event_text(e)))));
    let l = log.clone();
    tool.on_pointer_up =
        Some(Rc::new(move |e| l.borrow_mut().push(format!("up {}", event_text(e)))));
    let l = log.clone();
    tool.on_hover = Some(Rc::new(move |e| l.borrow_mut().push(format!("hover {}", event_text(e)))));
    tool
}

#[test]
fn trait_defaults_are_the_absent_member_behaviour() {
    let kernel = kernel();
    let hub = hub(&kernel, "a");
    let log = Log::default();
    hub.register_handler(Rc::new(UpOnly(log.clone())), RegisterHandlerOptions::default());
    assert!(!hub.would_claim_touch(&sample(Phase::Down)));
    hub.dispatch_pointer(&sample(Phase::Down));
    hub.dispatch_pointer(&sample(Phase::Move));
    hub.dispatch_pointer(&sample(Phase::Cancel));
    hub.dispatch_pointer(&sample(Phase::Up));
    assert_eq!(*log.borrow(), ["down", "up"]);
    hub.activate_tool("pan").unwrap();
    hub.dispatch_pointer(&sample(Phase::Down));
    assert_eq!(*log.borrow(), ["down", "up"]);
}

/// `ownPointerHandler`: a tool's own members get page-space events, keep
/// the page the gesture started on, take the press before every plugin
/// handler (priority 1000), and are live only while the tool is active.
#[test]
fn a_tools_own_pointer_members_route_in_page_space() {
    let kernel = kernel();
    let hub = hub(&kernel, "a");
    let log = Log::default();
    let plugin = Log::default();
    hub.register_tool(drawing_tool("ink", &log, true), RegisterToolOptions::default()).unwrap();

    struct Greedy(Log);
    impl InteractionHandler for Greedy {
        fn id(&self) -> &str {
            "greedy"
        }
        fn priority(&self) -> f64 {
            999.0
        }
        fn enabled_for(&self, _: &Tool) -> bool {
            true
        }
        fn on_down(&self, _: &PointerSample) -> bool {
            self.0.borrow_mut().push("greedy down".into());
            true
        }
    }
    hub.register_handler(Rc::new(Greedy(plugin.clone())), RegisterHandlerOptions::default());

    // Not active: the plugin handler takes the press.
    hub.dispatch_pointer(&on(Phase::Down, Some(hit(1, 5.0, 6.0))));
    hub.dispatch_pointer(&on(Phase::Up, Some(hit(1, 5.0, 6.0))));
    assert!(log.borrow().is_empty());
    assert_eq!(*plugin.borrow(), ["greedy down"]);

    hub.activate_tool("ink").unwrap();
    let started = Log::default();
    let s = started.clone();
    hub.on_gesture_started(move |e| {
        s.borrow_mut().push(format!("{} {:?}", e.tool_id, e.page));
        Ok(())
    });
    hub.dispatch_pointer(&on(Phase::Move, Some(hit(1, 1.0, 2.0))));
    hub.dispatch_pointer(&on(Phase::Down, Some(hit(1, 5.0, 6.0))));
    // Onto page 2: no projection, a different page, so the last event repeats.
    hub.dispatch_pointer(&on(Phase::Move, Some(hit(2, 7.0, 8.0))));
    // A projection speaks for the home page, off its box.
    let mut projected = on(Phase::Move, Some(hit(2, 7.0, 8.0)));
    projected.project =
        Some(Rc::new(|page| (page == PageRef::new(1)).then_some(Point { x: -3.0, y: 900.0 })));
    hub.dispatch_pointer(&projected);
    hub.dispatch_pointer(&on(Phase::Up, None));
    assert_eq!(
        *log.borrow(),
        ["hover 1@1,2", "down 1@5,6", "move 1@5,6", "move 1@-3,900", "up 1@-3,900"]
    );
    assert_eq!(*started.borrow(), ["ink Some(PageRef { page_object_number: 1 })"]);
    assert_eq!(*plugin.borrow(), ["greedy down"], "the tool took the press first");

    // A cancel ends the tool's gesture through on_pointer_up too.
    log.borrow_mut().clear();
    hub.dispatch_pointer(&on(Phase::Down, Some(hit(2, 1.0, 1.0))));
    hub.dispatch_pointer(&on(Phase::Cancel, Some(hit(2, 2.0, 2.0))));
    assert_eq!(*log.borrow(), ["down 2@1,1", "up 2@2,2"]);
}

/// A press the tool declines (anything but `true`) falls through to the
/// plugin handlers; a tool with neither `on_pointer_down` nor `on_hover`
/// brings no handler at all.
#[test]
fn a_declined_press_falls_through_and_a_plain_tool_brings_no_handler() {
    let kernel = kernel();
    let hub = hub(&kernel, "a");
    let log = Log::default();
    hub.register_tool(drawing_tool("ink", &log, false), RegisterToolOptions::default()).unwrap();
    let mut plain = Tool::new("plain", "cell", ["text-select"]);
    plain.on_pointer_up = Some(Rc::new(|_| panic!("no handler: never called")));
    hub.register_tool(plain, RegisterToolOptions::default()).unwrap();
    let up = Log::default();
    hub.register_handler(Rc::new(UpOnly(up.clone())), RegisterHandlerOptions::default());

    hub.activate_tool("ink").unwrap();
    hub.dispatch_pointer(&on(Phase::Down, Some(hit(1, 5.0, 6.0))));
    hub.dispatch_pointer(&on(Phase::Up, Some(hit(1, 5.0, 6.0))));
    assert_eq!(*log.borrow(), ["down 1@5,6"]);
    assert!(up.borrow().is_empty(), "ink doesn't enable text-select");

    hub.activate_tool("plain").unwrap();
    hub.dispatch_pointer(&on(Phase::Down, Some(hit(1, 5.0, 6.0))));
    hub.dispatch_pointer(&on(Phase::Up, Some(hit(1, 5.0, 6.0))));
    assert_eq!(*up.borrow(), ["down", "up"]);
}

/// `registerTool` with `replace` takes the old registration out (its own
/// handler with it) and puts the new one last; the old remover is inert and
/// the new one removes the tool and its cursors.
#[test]
fn replacing_a_registration_moves_it_last_and_retires_the_old_remover() {
    let kernel = kernel();
    let hub = hub(&kernel, "a");
    let log = Log::default();
    let first =
        hub.register_tool(drawing_tool("ink", &log, true), RegisterToolOptions::default()).unwrap();
    let conflict = hub
        .register_tool(Tool::new("ink", "cell", []), RegisterToolOptions::default())
        .err()
        .unwrap();
    assert_eq!(conflict.code(), Some("conflict"));
    let listed = hub.list_tools();
    assert!(Rc::ptr_eq(&listed, &hub.list_tools()), "the same list until the registry changes");

    let second = hub
        .register_tool(
            Tool::new("pointer", "default", ["text-select"]),
            RegisterToolOptions { replace: true },
        )
        .unwrap();
    let ids: Vec<String> = hub.list_tools().iter().map(|t| t.id.clone()).collect();
    assert_eq!(ids, ["pan", "ink", "pointer"]);
    assert!(!Rc::ptr_eq(&listed, &hub.list_tools()));

    let replacement = hub
        .register_tool(Tool::new("ink", "cell", ["ink"]), RegisterToolOptions { replace: true })
        .unwrap();
    hub.activate_tool("ink").unwrap();
    hub.dispatch_pointer(&on(Phase::Down, Some(hit(1, 5.0, 6.0))));
    assert!(log.borrow().is_empty(), "the replaced tool's own handler went with it");

    first.call();
    assert!(hub.has_tool("ink"), "the replaced registration's remover is inert");
    hub.set_tool_cursor("ink", Some([("cell".to_string(), "url(c.svg), cell".to_string())].into()));
    hub.dispatch_pointer(&on(Phase::Move, Some(hit(1, 1.0, 1.0))));
    assert_eq!(hub.get_cursor(), "url(c.svg), cell");
    replacement.call();
    assert!(!hub.has_tool("ink"));
    assert_eq!(hub.get_active_tool().id, "ink", "the active id stays, read as a neutral tool");
    hub.register_tool(Tool::new("ink", "cell", []), RegisterToolOptions::default()).unwrap();
    hub.activate_tool("ink").unwrap();
    assert_eq!(hub.get_cursor(), "cell", "the remover took the tool's cursors with it");
    second.call();
    assert!(!hub.has_tool("pointer"));
}

/// Taking a handler out mid-gesture releases the gesture: the up reaches
/// nobody and no gesture ends. A tool switch drops the owner and claims.
#[test]
fn removing_or_switching_away_from_the_owner_releases_the_gesture() {
    let kernel = kernel();
    let hub = hub(&kernel, "a");
    let log = Log::default();
    let ended = Log::default();
    let e = ended.clone();
    hub.on_gesture_ended(move |_| {
        e.borrow_mut().push("ended".into());
        Ok(())
    });
    let off = hub.register_handler(Rc::new(UpOnly(log.clone())), RegisterHandlerOptions::default());
    hub.dispatch_pointer(&sample(Phase::Down));
    off.call();
    hub.dispatch_pointer(&sample(Phase::Up));
    assert_eq!(*log.borrow(), ["down"]);
    assert!(ended.borrow().is_empty());

    hub.register_handler(Rc::new(UpOnly(log.clone())), RegisterHandlerOptions::default());
    hub.dispatch_pointer(&sample(Phase::Down));
    hub.claim_cursor("hover", Some("text"), 1.0);
    assert!(hub.has_cursor_claim());
    hub.activate_tool("pointer").unwrap();
    assert!(!hub.has_cursor_claim());
    hub.dispatch_pointer(&sample(Phase::Up));
    assert_eq!(*log.borrow(), ["down", "down"]);
    assert!(ended.borrow().is_empty());
}

/// The vectors' page refs have no `objectNumber`, so their same-page cases
/// answer `null`. With a real ref, the page hit speaks for its own page.
#[test]
fn sample_point_on_falls_back_to_the_same_page_hit() {
    let mut s = on(Phase::Move, Some(hit(3, 7.0, 8.0)));
    assert_eq!(sample_point_on(&s, PageRef::new(3)), Some(Point { x: 7.0, y: 8.0 }));
    assert_eq!(sample_point_on(&s, PageRef::new(4)), None);
    s.project =
        Some(Rc::new(|page| (page == PageRef::new(4)).then_some(Point { x: 0.0, y: -1.0 })));
    assert_eq!(sample_point_on(&s, PageRef::new(4)), Some(Point { x: 0.0, y: -1.0 }));
    assert_eq!(sample_point_on(&s, PageRef::new(3)), Some(Point { x: 7.0, y: 8.0 }));
}

/// The `tools` and `defaultTool` settings belong to the registration: every
/// document opens with them, a change reaches every open document, and
/// `reset_settings` goes back to what the app registered.
#[test]
fn settings_tools_are_registered_and_swapped_for_every_document() {
    let log = Log::default();
    let ink: Rc<Tool> = Rc::new(drawing_tool("ink", &log, true));
    let kernel = kernel_with(InteractionConfig {
        default_tool: Some("ink".into()),
        tools: Some(vec![ink.clone()]),
    });
    let (a, b) = (hub(&kernel, "a"), hub(&kernel, "b"));
    for hub in [&a, &b] {
        assert_eq!(hub.get_active_tool_id(), "ink");
        assert!(Rc::ptr_eq(&hub.get_tool("ink").unwrap(), &ink), "the setting's own tool");
    }
    let changes = Log::default();
    let c = changes.clone();
    b.on_settings_changed(move |e| {
        c.borrow_mut().push(format!("{:?} {}", e.changed, e.settings.default_tool));
        Ok(())
    });

    let shape: Rc<Tool> = Rc::new(Tool::new("shape", "crosshair", ["shape"]));
    a.update_settings(&InteractionConfig {
        default_tool: Some("shape".into()),
        tools: Some(vec![shape.clone()]),
    });
    for hub in [&a, &b] {
        assert!(!hub.has_tool("ink"));
        assert!(Rc::ptr_eq(&hub.get_tool("shape").unwrap(), &shape));
        assert_eq!(hub.get_default_tool_id(), "shape");
        assert_eq!(hub.get_active_tool_id(), "ink", "a setting doesn't switch the active tool");
    }
    assert_eq!(*changes.borrow(), ["[\"defaultTool\", \"tools\"] shape"]);

    a.update_settings(&InteractionConfig { default_tool: None, tools: Some(vec![shape.clone()]) });
    assert_eq!(changes.borrow().len(), 1, "the same tool is no change");

    b.reset_settings();
    assert!(Rc::ptr_eq(&a.get_tool("ink").unwrap(), &ink));
    assert!(!a.has_tool("shape"));
    assert_eq!(a.get_default_tool_id(), "ink");
    let settings = kernel.settings_of(INTERACTION.any()).unwrap().get_settings();
    assert_eq!(settings["defaultTool"], "ink");
}

/// Kept divergence: a settings tool is compared by identity. Two distinct
/// tools with equal data and no callbacks are a change here, so the swap
/// runs and its removers drop the tools' `set_tool_cursor` entries. The
/// TypeScript's `sameValue` (`packages/core/main/src/settings.ts:187-222`)
/// deep-compares a plain tool given without `enables` and calls this no
/// change; `Tool.enables` can't tell that input from an empty set.
#[test]
fn distinct_equal_settings_tools_count_as_a_change() {
    let kernel = kernel();
    let a = hub(&kernel, "a");
    let changes = Log::default();
    let c = changes.clone();
    a.on_settings_changed(move |e| {
        c.borrow_mut().push(format!("{:?}", e.changed));
        Ok(())
    });
    let tool = || Rc::new(Tool::new("note", "cell", []));
    let first = tool();
    a.update_settings(&InteractionConfig { default_tool: None, tools: Some(vec![first.clone()]) });
    a.update_settings(&InteractionConfig { default_tool: None, tools: Some(vec![first]) });
    assert_eq!(*changes.borrow(), ["[\"tools\"]"], "the same tool is no change");

    a.activate_tool("note").unwrap();
    a.dispatch_pointer(&on(Phase::Move, Some(hit(1, 1.0, 1.0))));
    a.set_tool_cursor("note", Some([("cell".to_string(), "url(n.svg), cell".to_string())].into()));
    assert_eq!(a.get_cursor(), "url(n.svg), cell");

    a.update_settings(&InteractionConfig { default_tool: None, tools: Some(vec![tool()]) });
    assert_eq!(*changes.borrow(), ["[\"tools\"]", "[\"tools\"]"], "an equal tool is a change");
    a.activate_tool("note").unwrap();
    assert_eq!(a.get_cursor(), "cell", "the swap's remover dropped the tool's cursors");
}

/// A JSON write through the registration's store releases the typed tools
/// earlier settings held, as a typed change does.
#[test]
fn a_json_settings_write_releases_earlier_typed_tools() {
    let kernel = kernel();
    let (a, b) = (hub(&kernel, "a"), hub(&kernel, "b"));
    let typed = Rc::new(Tool::new("typed", "cell", []));
    let weak = Rc::downgrade(&typed);
    a.update_settings(&InteractionConfig { default_tool: None, tools: Some(vec![typed]) });
    assert!(b.has_tool("typed"));
    kernel
        .settings_of(INTERACTION.any())
        .unwrap()
        .update_settings(&json!({ "tools": [{ "id": "plain", "cursor": "copy" }] }));
    for hub in [&a, &b] {
        assert!(!hub.has_tool("typed"));
        assert_eq!(hub.get_tool("plain").unwrap().cursor, "copy");
        hub.list_tools();
    }
    assert!(weak.upgrade().is_none(), "nothing keeps the earlier typed tool");
}

/// Kept divergence: the hub releases its handlers, their scopes, the
/// gesture owner and the tools' own handlers when the document closes,
/// which the TypeScript leaves to the garbage collector. A closed hub then
/// routes nothing, and its removers are harmless.
#[test]
fn closing_the_document_releases_the_handlers() {
    let kernel = kernel();
    let hub = hub(&kernel, "a");
    let log = Log::default();
    let handler: Rc<dyn InteractionHandler> = Rc::new(UpOnly(log.clone()));
    let off = hub
        .register_handler(handler.clone(), RegisterHandlerOptions { source: Some("lens".into()) });
    let tool_log = Log::default();
    let tool_off = hub
        .register_tool(drawing_tool("ink", &tool_log, true), RegisterToolOptions::default())
        .unwrap();
    hub.dispatch_pointer(&sample(Phase::Down));
    let seen = Rc::new(RefCell::new(0));
    let s = seen.clone();
    hub.on_tool_changed(move |_| {
        *s.borrow_mut() += 1;
        Ok(())
    });
    // This test and the remover hold it; so does the hub, until the close.
    assert_eq!(Rc::strong_count(&handler), 5, "plus the list, the source scope and the owner");
    let _ = kernel.documents().close("a");
    rt::run_until_stalled();
    assert_eq!(Rc::strong_count(&handler), 2, "only this test and the remover");

    hub.dispatch_pointer(&sample(Phase::Up));
    hub.dispatch_pointer(&sample(Phase::Down));
    assert_eq!(*log.borrow(), ["down"], "nothing routes after the close");
    off.call();
    tool_off.call();
    assert_eq!(*seen.borrow(), 0);
    // The other document's hub is untouched.
    let b = self::hub(&kernel, "b");
    b.register_handler(Rc::new(UpOnly(log.clone())), RegisterHandlerOptions::default());
    b.dispatch_pointer(&sample(Phase::Down));
    assert_eq!(*log.borrow(), ["down", "down"]);
}

#[test]
fn the_state_read_is_empty_without_a_document() {
    let kernel = kernel();
    let hub = hub(&kernel, "a");
    let read = interaction_state(Some(&hub));
    assert_eq!(read.active_tool_id.as_deref(), Some("pointer"));
    assert!(Rc::ptr_eq(&read.tools, &hub.list_tools()));
    let empty = interaction_state(None);
    assert_eq!(empty.active_tool_id, None);
    assert!(empty.tools.is_empty());
}

#[test]
fn a_sample_debug_prints_without_its_function() {
    let mut s = sample(Phase::Move);
    s.project = Some(Rc::new(|_| None));
    let text = format!("{s:?}");
    assert!(text.starts_with("PointerSample { phase: Move"), "{text}");
    assert!(text.ends_with("project: Some(\"fn\") }"), "{text}");
}

#[test]
fn tool_tags_are_a_set() {
    let tool = Tool::new("t", "default", ["a", "b", "a"]);
    assert_eq!(tool.enables, ["a", "b"]);
    assert!(tool.enables("b"));
    assert!(!tool.enables("c"));
}

/// A tool written as JSON reads its data members, `enables` as a list or
/// a canonical set, and no callbacks.
#[test]
fn a_plain_tool_reads_its_data_members() {
    let tool = plain_tool(&json!({
        "id": "ink", "cursor": "crosshair", "touch": "draw", "gapCursor": "not-allowed",
        "enables": { "$set": ["ink", "draw", "ink"] }
    }))
    .unwrap();
    assert_eq!(tool.enables, ["ink", "draw"]);
    assert_eq!(tool.touch, Some(ToolTouch::Draw));
    assert_eq!(tool.gap_cursor.as_deref(), Some("not-allowed"));
    assert!(!tool.handles_pointer());
    assert_eq!(plain_tool(&json!({ "id": "x", "enables": ["a"] })).unwrap().enables, ["a"]);
    assert!(plain_tool(&json!({ "cursor": "x" })).is_none());
}
