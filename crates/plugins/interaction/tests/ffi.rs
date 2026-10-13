#![cfg(feature = "uniffi")]

use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use epdf_ffi::{
    HostError, KernelThread, Modifiers, PageRef, Point, PointerKind, PointerPageHit, PointerPhase,
    PointerSample, ViewerError,
};
use epdf_plugin_interaction::ffi::{
    InteractionEvent, InteractionEventListener, InteractionHandle, InteractionHandler,
    InteractionTool,
};
use epdf_plugin_interaction::{INTERACTION, InteractionConfig, interaction_plugin};
use futures::executor::block_on;
use plugin_conformance::test_kernel::{TestKernel, one_document};

#[derive(Default)]
struct Events(Mutex<Vec<InteractionEvent>>, AtomicUsize);

impl InteractionEventListener for Events {
    fn on_event(&self, event: InteractionEvent) {
        self.0.lock().unwrap().push(event);
    }
    fn on_end(&self) {
        self.1.fetch_add(1, Ordering::SeqCst);
    }
}

/// Logs every call; captures on down unless told to throw.
#[derive(Default)]
struct Recording {
    log: Mutex<Vec<String>>,
    throws: bool,
    claims_touch: bool,
}

impl InteractionHandler for Recording {
    fn enabled_for(&self, tool: InteractionTool) -> bool {
        tool.enables.iter().any(|t| t == "text-select")
    }
    fn on_down(&self, sample: PointerSample) -> Result<bool, HostError> {
        self.log.lock().unwrap().push(format!("down {}", sample.viewport.x));
        if self.throws {
            return Err(HostError::Failed { detail: "handler failed".into() });
        }
        Ok(true)
    }
    fn on_move(&self, sample: PointerSample) {
        self.log.lock().unwrap().push(format!("move {}", sample.viewport.x));
    }
    fn on_up(&self, sample: PointerSample) {
        self.log.lock().unwrap().push(format!("up {}", sample.viewport.x));
    }
    fn on_cancel(&self, sample: PointerSample) {
        self.log.lock().unwrap().push(format!("cancel {}", sample.viewport.x));
    }
    fn on_hover(&self, sample: PointerSample) {
        self.log.lock().unwrap().push(format!("hover {}", sample.viewport.x));
    }
    fn claims_touch(&self, _sample: PointerSample) -> bool {
        self.claims_touch
    }
}

type Reports = Arc<Mutex<Vec<String>>>;

fn handle() -> (KernelThread<TestKernel>, Arc<InteractionHandle>, Reports) {
    let thread = KernelThread::spawn(|_| {
        TestKernel::open(vec![interaction_plugin(InteractionConfig::default())], one_document())
    })
    .unwrap();
    let post = thread.post();
    let reports: Reports = Arc::default();
    let sink = reports.clone();
    let handle = block_on(thread.call(move |k| {
        let report = Rc::new(move |e: &epdf_kernel::Error| {
            sink.lock().unwrap().push(format!("{}: {}", e.name, e.message));
        });
        InteractionHandle::new(k.capability(&INTERACTION).unwrap(), post, report)
    }))
    .unwrap();
    (thread, handle, reports)
}

fn sample(phase: PointerPhase, x: f64) -> PointerSample {
    PointerSample {
        phase,
        viewport: Point { x, y: 10.0 },
        page: Some(PointerPageHit {
            page: PageRef { page_object_number: 1 },
            point: Point { x, y: 10.0 },
            scale: None,
            rotation: None,
            zoom: None,
        }),
        modifiers: Modifiers::default(),
        click_count: None,
        pointer_type: Some(PointerKind::Touch),
        gesture: None,
        source: None,
    }
}

fn code(error: &ViewerError) -> Option<&str> {
    match error {
        ViewerError::Kernel { code, .. } => code.as_deref(),
        ViewerError::Closed => None,
    }
}

/// Let every job posted so far run.
fn drain(thread: &KernelThread<TestKernel>) {
    block_on(thread.call(|_| ())).unwrap();
}

#[test]
fn the_built_in_tools_are_listed_and_pointer_is_active() {
    let (_thread, interaction, _) = handle();
    let ids: Vec<String> =
        block_on(interaction.tools()).unwrap().into_iter().map(|t| t.id).collect();
    assert_eq!(ids, ["pointer", "pan"]);
    assert_eq!(block_on(interaction.active_tool_id()).unwrap(), "pointer");
    assert_eq!(block_on(interaction.default_tool_id()).unwrap(), "pointer");
    assert!(block_on(interaction.active_tool_enables("text-select".into())).unwrap());
}

#[test]
fn activating_switches_the_tool_and_is_observed() {
    let (thread, interaction, _) = handle();
    let events = Arc::new(Events::default());
    let _sub = interaction.subscribe(events.clone());
    block_on(interaction.activate_tool("pan".into())).unwrap();
    drain(&thread);
    assert_eq!(block_on(interaction.active_tool()).unwrap().cursor, "grab");
    let seen = events.0.lock().unwrap();
    // The hub re-resolves the cursor before it announces the tool, so the
    // cursor change comes first.
    let tool_changed = seen.iter().find(|e| matches!(e, InteractionEvent::ToolChanged { .. }));
    assert!(
        matches!(tool_changed, Some(InteractionEvent::ToolChanged { event })
            if event.tool_id == "pan" && event.previous_tool_id == "pointer"),
        "{seen:?}"
    );
}

#[test]
fn an_unknown_tool_is_not_found() {
    let (_thread, interaction, _) = handle();
    let error = block_on(interaction.activate_tool("laser".into())).unwrap_err();
    assert_eq!(code(&error), Some("not-found"), "{error:?}");
}

#[test]
fn push_and_pop_restore_the_previous_tool() {
    let (_thread, interaction, _) = handle();
    block_on(interaction.push_tool("pan".into())).unwrap();
    assert_eq!(block_on(interaction.active_tool_id()).unwrap(), "pan");
    block_on(interaction.pop_tool()).unwrap();
    assert_eq!(block_on(interaction.active_tool_id()).unwrap(), "pointer");
}

#[test]
fn a_registered_tool_conflicts_until_replaced_and_unregisters() {
    let (thread, interaction, _) = handle();
    let tool = InteractionTool {
        id: "ink".into(),
        cursor: "crosshair".into(),
        gap_cursor: None,
        enables: vec!["annotation-create".into()],
        touch_direct: Some(true),
    };
    let registration = block_on(interaction.register_tool(tool.clone(), false)).unwrap();
    let error = block_on(interaction.register_tool(tool.clone(), false)).err().unwrap();
    assert_eq!(code(&error), Some("conflict"), "{error:?}");
    let replaced = block_on(interaction.register_tool(tool, true)).unwrap();
    // The first registration no longer owns the id, so its unregister is a no-op.
    registration.unregister();
    drain(&thread);
    assert!(block_on(interaction.has_tool("ink".into())).unwrap());
    replaced.unregister();
    drain(&thread);
    assert!(!block_on(interaction.has_tool("ink".into())).unwrap());
}

#[test]
fn a_tool_cursor_skin_changes_the_resolved_cursor() {
    let (thread, interaction, _) = handle();
    let events = Arc::new(Events::default());
    let _sub = interaction.subscribe(events.clone());
    // A skin applies over a page; over a gap the tool's gap cursor wins.
    interaction.dispatch_pointer(sample(PointerPhase::Move, 1.0));
    block_on(interaction.set_tool_cursor(
        "pointer".into(),
        Some([("default".to_string(), "pointer".to_string())].into()),
    ))
    .unwrap();
    drain(&thread);
    assert_eq!(block_on(interaction.cursor()).unwrap(), "pointer");
    assert!(events.0.lock().unwrap().iter().any(|e| matches!(e,
        InteractionEvent::CursorChanged { event } if event.cursor == "pointer")));
}

#[test]
fn a_cursor_claim_wins_by_priority_and_clears() {
    let (_thread, interaction, _) = handle();
    block_on(interaction.claim_cursor("drag".into(), Some("grabbing".into()), 10.0)).unwrap();
    assert_eq!(block_on(interaction.cursor()).unwrap(), "grabbing");
    block_on(interaction.claim_cursor("drag".into(), None, 0.0)).unwrap();
    assert_eq!(block_on(interaction.cursor()).unwrap(), "default");
}

#[test]
fn a_foreign_handler_captures_a_gesture() {
    let (thread, interaction, _) = handle();
    let events = Arc::new(Events::default());
    let _sub = interaction.subscribe(events.clone());
    let handler = Arc::new(Recording::default());
    let _registration =
        block_on(interaction.register_handler("recording".into(), 5.0, None, handler.clone()))
            .unwrap();
    interaction.dispatch_pointer(sample(PointerPhase::Down, 1.0));
    interaction.dispatch_pointer(sample(PointerPhase::Move, 2.0));
    interaction.dispatch_pointer(sample(PointerPhase::Up, 3.0));
    drain(&thread);
    assert_eq!(*handler.log.lock().unwrap(), ["down 1", "move 2", "up 3"]);
    let seen = events.0.lock().unwrap();
    let kinds: Vec<&str> = seen
        .iter()
        .map(|e| match e {
            InteractionEvent::GestureStarted { .. } => "started",
            InteractionEvent::GestureEnded { .. } => "ended",
            InteractionEvent::GestureCancelled { .. } => "cancelled",
            InteractionEvent::ToolChanged { .. } => "tool",
            InteractionEvent::CursorChanged { .. } => "cursor",
        })
        .filter(|k| *k != "cursor")
        .collect();
    assert_eq!(kinds, ["started", "ended"]);
    assert!(matches!(&seen.iter().find(|e| matches!(e, InteractionEvent::GestureStarted { .. })),
        Some(InteractionEvent::GestureStarted { event })
            if event.handler_id == "recording"
                && event.page == Some(PageRef { page_object_number: 1 })
                && event.pointer_type == PointerKind::Touch));
}

#[test]
fn a_throwing_handler_is_reported_and_does_not_capture() {
    let (thread, interaction, reports) = handle();
    let events = Arc::new(Events::default());
    let _sub = interaction.subscribe(events.clone());
    let handler = Arc::new(Recording { throws: true, ..Default::default() });
    let _registration =
        block_on(interaction.register_handler("throws".into(), 5.0, None, handler.clone()))
            .unwrap();
    interaction.dispatch_pointer(sample(PointerPhase::Down, 1.0));
    interaction.dispatch_pointer(sample(PointerPhase::Move, 2.0));
    drain(&thread);
    let reports = reports.lock().unwrap();
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert!(reports[0].starts_with("HostError: "), "{reports:?}");
    assert!(reports[0].contains("handler failed"), "{reports:?}");
    // Not captured: the move hovers instead of reaching `on_move`.
    assert_eq!(*handler.log.lock().unwrap(), ["down 1", "hover 2"]);
    assert!(
        !events
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, InteractionEvent::GestureStarted { .. }))
    );
}

#[test]
fn samples_arrive_in_the_order_they_were_sent() {
    let (thread, interaction, _) = handle();
    let handler = Arc::new(Recording::default());
    let _registration =
        block_on(interaction.register_handler("order".into(), 1.0, None, handler.clone())).unwrap();
    interaction.dispatch_pointer(sample(PointerPhase::Down, 0.0));
    for x in 1..=1000 {
        interaction.dispatch_pointer(sample(PointerPhase::Move, f64::from(x)));
    }
    interaction.dispatch_pointer(sample(PointerPhase::Up, 1001.0));
    drain(&thread);
    let log = handler.log.lock().unwrap();
    assert_eq!(log.len(), 1002);
    for (i, entry) in log.iter().enumerate().skip(1).take(1000) {
        assert_eq!(*entry, format!("move {i}"));
    }
}

#[test]
fn unregistering_the_capturing_handler_mid_gesture_releases_it() {
    let (thread, interaction, _) = handle();
    let handler = Arc::new(Recording::default());
    let registration =
        block_on(interaction.register_handler("gone".into(), 1.0, None, handler.clone())).unwrap();
    interaction.dispatch_pointer(sample(PointerPhase::Down, 1.0));
    drain(&thread);
    registration.unregister();
    interaction.dispatch_pointer(sample(PointerPhase::Move, 2.0));
    interaction.dispatch_pointer(sample(PointerPhase::Up, 3.0));
    drain(&thread);
    assert_eq!(*handler.log.lock().unwrap(), ["down 1"]);
}

#[test]
fn would_claim_touch_asks_the_eligible_handlers() {
    let (_thread, interaction, _) = handle();
    assert!(!block_on(interaction.would_claim_touch(sample(PointerPhase::Down, 1.0))).unwrap());
    let handler = Arc::new(Recording { claims_touch: true, ..Default::default() });
    let _registration =
        block_on(interaction.register_handler("claims".into(), 1.0, None, handler)).unwrap();
    assert!(block_on(interaction.would_claim_touch(sample(PointerPhase::Down, 1.0))).unwrap());
}

#[test]
fn a_handler_scoped_to_a_source_ignores_other_sources() {
    let (thread, interaction, _) = handle();
    let handler = Arc::new(Recording::default());
    let _registration = block_on(interaction.register_handler(
        "scoped".into(),
        1.0,
        Some("thumbnails".into()),
        handler.clone(),
    ))
    .unwrap();
    let mut down = sample(PointerPhase::Down, 1.0);
    down.source = Some("stage".into());
    interaction.dispatch_pointer(down);
    drain(&thread);
    assert!(handler.log.lock().unwrap().is_empty());
}

#[test]
fn a_closed_documents_capability_refuses_with_instance_closed() {
    let (thread, interaction, _) = handle();
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    for error in [
        block_on(interaction.active_tool_id()).map(|_| ()).unwrap_err(),
        block_on(interaction.activate_tool("pan".into())).unwrap_err(),
        block_on(interaction.cursor()).map(|_| ()).unwrap_err(),
    ] {
        assert_eq!(code(&error), Some("instance-closed"), "{error:?}");
    }
    // A sample after the close is dropped, not a panic.
    interaction.dispatch_pointer(sample(PointerPhase::Down, 1.0));
    drain(&thread);
}

#[test]
fn closing_the_document_ends_its_subscriptions() {
    let (thread, interaction, _) = handle();
    let events = Arc::new(Events::default());
    let _subscription = interaction.subscribe(events.clone());
    drain(&thread);
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    assert_eq!(events.1.load(Ordering::SeqCst), 1);
    let late = Arc::new(Events::default());
    let _late = interaction.subscribe(late.clone());
    drain(&thread);
    assert_eq!(late.1.load(Ordering::SeqCst), 1);
}

use epdf_plugin_interaction::ffi::{
    FeedbackConfig, FeedbackHandle, FeedbackImpactWeight, FeedbackNotifyKind, PlatformFeedback,
};
use epdf_plugin_interaction::{FEEDBACK, feedback_plugin};

#[derive(Default)]
struct Haptics(Mutex<Vec<String>>);

impl PlatformFeedback for Haptics {
    fn selection(&self) {
        self.0.lock().unwrap().push("selection".into());
    }
    fn impact(&self, weight: Option<FeedbackImpactWeight>) {
        self.0.lock().unwrap().push(format!("impact {weight:?}"));
    }
    fn notify(&self, kind: FeedbackNotifyKind) {
        self.0.lock().unwrap().push(format!("notify {kind:?}"));
    }
}

#[test]
fn feedback_reaches_the_foreign_provider() {
    let haptics = Arc::new(Haptics::default());
    let config = FeedbackConfig { provider: haptics.clone() };
    let thread = KernelThread::spawn(move |_| {
        TestKernel::open(vec![feedback_plugin(config.into_options())], one_document())
    })
    .unwrap();
    let post = thread.post();
    let feedback =
        block_on(thread.call(move |k| {
            FeedbackHandle::new(k.kernel.capability(&FEEDBACK, None).unwrap(), post)
        }))
        .unwrap();
    block_on(feedback.selection()).unwrap();
    block_on(feedback.impact(Some(FeedbackImpactWeight::Heavy))).unwrap();
    block_on(feedback.impact(None)).unwrap();
    block_on(feedback.notify(FeedbackNotifyKind::Warning)).unwrap();
    assert_eq!(
        *haptics.0.lock().unwrap(),
        ["selection", "impact Some(Heavy)", "impact None", "notify Warning"]
    );
}
