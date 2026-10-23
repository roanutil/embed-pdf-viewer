use epdf_kernel::{engine::Engine, rt};
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use serde_json::json;
use std::{cell::RefCell, rc::Rc};

#[test]
fn confirmed_events_precede_settlement_and_stay_with_the_opened_handle() {
    rt::reset();
    let fake = FakeEngine::new(json!({"documents":{"a":{"pages":[],"responses":[
        {"path":"apply","defer":"apply","events":[{"type":"changed","origin":{"tx":"op","serverId":1}}]}
    ]}}}));
    let open = AsEngine(fake.clone()).open(json!({"id":"a"}), None);
    let first = open.peek().unwrap().unwrap();
    let log = Rc::new(RefCell::new(Vec::new()));
    let sink = log.clone();
    let _off = first.subscribe(Rc::new(move |_| sink.borrow_mut().push("event")));
    let call = first.call("apply", None, vec![]);
    let sink = log.clone();
    call.on_settle(move |_| sink.borrow_mut().push("settled"));
    let second = AsEngine(fake.clone()).open(json!({"id":"a"}), None).peek().unwrap().unwrap();
    let _off = second.subscribe(Rc::new(move |_| panic!("old call leaked into reopened handle")));
    assert!(fake.settle("apply", Ok(json!(null))));
    assert_eq!(*log.borrow(), vec!["event", "settled"]);
}

#[test]
fn scenario_frames_wait_sixteen_ms_and_cancel_with_lifetime() {
    rt::reset();
    let lifetime = epdf_kernel::signal::AbortSignal::new();
    let clock = epdf_kernel::clock::PluginClock::new(
        Rc::new(plugin_conformance::scenario::ScenarioClock { frames: true }),
        lifetime.clone(),
    );
    let fired = Rc::new(RefCell::new(Vec::new()));
    let sink = fired.clone();
    clock.next_frame(move |time| sink.borrow_mut().push(time));
    rt::advance(15.0);
    assert!(fired.borrow().is_empty());
    rt::advance(1.0);
    assert_eq!(*fired.borrow(), vec![16.0]);
    let sink = fired.clone();
    clock.next_frame(move |time| sink.borrow_mut().push(time));
    lifetime.abort(epdf_kernel::Error::new("closed"));
    rt::advance(16.0);
    assert_eq!(fired.borrow().len(), 1);
}

#[test]
fn reserved_numbers_are_per_open_and_never_reused_after_take() {
    rt::reset();
    let fake =
        FakeEngine::new(json!({"documents":{"a":{"local":true,"pages":[],"objectNumbers":[7]}}}));
    let doc = AsEngine(fake.clone()).open(json!({"id":"a"}), None).peek().unwrap().unwrap();
    assert!(doc.is_local());
    assert_eq!(doc.held_object_numbers(), 1);
    assert_eq!(doc.take_object_number().unwrap(), Some(7));
    doc.reserve_object_numbers(2);
    assert_eq!(doc.take_object_number().unwrap(), Some(8));
    assert_eq!(doc.take_object_number().unwrap(), Some(9));
    doc.reserve_object_numbers(1);
    assert_eq!(doc.take_object_number().unwrap(), Some(10));
    let reopened = AsEngine(fake).open(json!({"id":"a"}), None).peek().unwrap().unwrap();
    assert_eq!(reopened.take_object_number().unwrap(), Some(7));
}

#[test]
fn viewer_settings_merge_reset_supply_open_defaults_and_dispose_listeners() {
    rt::reset();
    let fake = FakeEngine::new(json!({"documents":{"a":{"pages":[]},"b":{"pages":[]}}}));
    let kernel = epdf_kernel::Kernel::new_with_viewer_settings(Rc::new(AsEngine(fake.clone())), vec![], Default::default(), Some(json!({"identity":{"name":"registered","extra":true},"scope":["doc.print"],"page":{"background":"red"}}))).unwrap();
    assert_eq!(kernel.get_settings()["accent"], "#3858e9");
    assert_eq!(kernel.get_settings()["page"]["shadow"], "0 6px 18px rgb(0 0 0 / 0.18)");
    kernel.update_settings(&json!({"identity":{"name":"replacement"}}));
    assert_eq!(kernel.get_settings()["identity"], json!({"name":"replacement"}));
    kernel.documents().open(epdf_kernel::OpenSource::Input(json!({"id":"a"})), Default::default());
    rt::run_until_stalled();
    assert_eq!(
        fake.log_since(0)[0]["args"][1],
        json!({"scope":["doc.print"],"identity":{"name":"replacement"}})
    );
    kernel.documents().open(
        epdf_kernel::OpenSource::Input(json!({"id":"b"})),
        epdf_kernel::OpenOptions::from_json(&json!({"identity":{"name":"own"},"scope":[]})),
    );
    rt::run_until_stalled();
    let calls = fake.log_since(0);
    let second = calls.iter().filter(|c| c["path"] == "open").nth(1).unwrap();
    assert_eq!(second["args"][1], json!({"identity":{"name":"own"},"scope":[]}));
    kernel.reset_settings();
    assert_eq!(kernel.get_settings()["identity"], json!({"name":"registered","extra":true}));
    let events = kernel.on_settings_changed();
    events.on(|_| panic!("listener must be disposed"));
    kernel.destroy();
    rt::run_until_stalled();
    kernel.update_settings(&json!({"accent":"blue"}));
    assert_eq!(kernel.get_settings()["accent"], "blue");
    assert_eq!(events.listener_count(), 0);
}

#[test]
fn dotted_getters_resolve_at_invocation_and_returned_methods_keep_the_receiver() {
    use plugin_conformance::{Harness, HarnessPlugin, Outcome};
    use std::any::Any;
    struct Root(Rc<RefCell<i64>>);
    struct Child(i64);
    struct Broken;
    struct Adapter;
    impl HarnessPlugin for Adapter {
        fn factories(&self) -> Vec<&'static str> {
            vec![]
        }
        fn build(
            &self,
            _: &str,
            _: &serde_json::Value,
            _: &Harness,
        ) -> Result<epdf_kernel::AnyPlugin, epdf_kernel::Error> {
            unreachable!()
        }
        fn resolve_member(
            &self,
            capability: &Rc<dyn Any>,
            name: &str,
            _: &Harness,
        ) -> Option<Result<Rc<dyn Any>, epdf_kernel::Error>> {
            if capability.is::<Broken>() {
                return Some(Err(epdf_kernel::Error::new("getter threw")));
            }
            let root = capability.downcast_ref::<Root>()?;
            if name != "current" {
                return None;
            }
            Some(Ok(Rc::new(Child(*root.0.borrow()))))
        }
        fn call(
            &self,
            capability: &Rc<dyn Any>,
            method: &str,
            args: Vec<serde_json::Value>,
            _: &Harness,
        ) -> Option<Outcome> {
            let child = capability.downcast_ref::<Child>()?;
            Some(if method == "add" {
                Outcome::Returned(json!(child.0 + args[0].as_i64().unwrap()))
            } else {
                Outcome::Threw(epdf_kernel::Error::new("missing"))
            })
        }
        fn members(&self, capability: &Rc<dyn Any>) -> Option<Vec<&'static str>> {
            if capability.is::<Root>() {
                Some(vec!["current"])
            } else if capability.is::<Child>() {
                Some(vec!["add"])
            } else if capability.is::<Broken>() {
                Some(vec!["current"])
            } else {
                None
            }
        }
    }
    let h = Harness::new(vec![Rc::new(Adapter)], Default::default());
    let state = Rc::new(RefCell::new(10));
    let root: Rc<dyn Any> = Rc::new(Root(state.clone()));
    let described = h.describe(root.clone()).unwrap();
    let bound = h.ref_fn(described["current"]["add"]["$ref"].as_u64().unwrap() as usize).unwrap();
    *state.borrow_mut() = 20;
    assert_eq!(h.outcome_json(h.call(&root, "current.add", vec![json!(2)]))["returned"], 22);
    assert_eq!(h.outcome_json(bound(vec![json!(2)]))["returned"], 12);
    let broken: Rc<dyn Any> = Rc::new(Broken);
    let error = h.try_describe(broken.clone()).unwrap().unwrap_err();
    assert_eq!(error.message, "getter threw");
    assert_eq!(
        h.outcome_json(h.call(&broken, "current.add", vec![json!(2)]))["threw"]["$error"]["message"],
        "getter threw"
    );
}

#[test]
fn fake_layout_and_cursor_follow_events_for_the_same_open() {
    rt::reset();
    let fake = FakeEngine::new(json!({"documents":{"a":{"pages":[]}}}));
    let doc = AsEngine(fake.clone()).open(json!({"id":"a"}), None).peek().unwrap().unwrap();
    fake.emit("a", &json!({"type":"pages.inserted","layout":{"pages":[{"index":0,"ref":{"kind":"objectNumber","objectNumber":9}}],"namedPages":["cover"]},"origin":{"serverId":5,"tx":null}})).unwrap();
    let layout = doc.call("pages.list", None, vec![]).peek().unwrap().unwrap();
    assert_eq!(layout["pageCount"], 1);
    assert_eq!(layout["namedPages"], json!(["cover"]));
    assert_eq!(doc.page_index(9).unwrap(), 0);
    assert_eq!(doc.last_server_id().as_deref(), Some("5"));
    fake.emit("a", &json!({"type":"changed","origin":{"serverId":3,"tx":null}})).unwrap();
    assert_eq!(doc.last_server_id().as_deref(), Some("5"));
}

#[test]
fn case_entropy_is_shared_across_uuid_versions_and_restored_on_unwind() {
    use epdf_kernel::identity::{generate_uuid, generate_uuid_v7, with_uuid_source};
    let _outer = with_uuid_source(Rc::new(|bytes| bytes.fill(0xff)), Rc::new(|| 123.0));
    let outer = generate_uuid();
    let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _case = plugin_conformance::scenario::install_case_entropy();
        assert_eq!(generate_uuid(), "00000000-0000-4000-8000-000000000001");
        assert_eq!(generate_uuid_v7(Some(256.0)), "00000000-0100-7000-8000-000000000002");
        {
            let _nested = plugin_conformance::scenario::install_case_entropy();
            assert_eq!(generate_uuid(), "00000000-0000-4000-8000-000000000001");
        }
        assert_eq!(generate_uuid(), "00000000-0000-4000-8000-000000000003");
        panic!("unwind case");
    }));
    assert!(failed.is_err());
    assert_eq!(generate_uuid(), outer);
    assert_eq!(&generate_uuid_v7(None)[..13], "00000000-007b");
}

#[test]
fn text_layout_hydration_uses_the_engine_geometry_methods() {
    use plugin_conformance::fake_engine::hydrate_text_layout;
    let layout = hydrate_text_layout(&json!({"$textLayout":{"runs":[]}})).unwrap();
    assert_eq!(layout.char_count(), 0);
    assert_eq!(layout.char_quad(0), None);
    let glyph = hydrate_text_layout(&json!({"$textLayout":{"runs":[{"start":0,"rect":{"x":10,"y":20,"width":6,"height":12},"fontSize":12,"glyphs":[{"loose":{"x":10,"y":20,"width":6,"height":12},"tight":{"x":11,"y":21,"width":4,"height":10}}]}]}})).unwrap();
    assert_eq!(glyph.char_count(), 1);
    assert!(glyph.char_quad(0).is_some());
    assert_eq!(glyph.char_quad(1), None);
    assert!(
        hydrate_text_layout(&json!({"runs":[]}))
            .unwrap_err()
            .message
            .contains("needs a $textLayout geometry fixture")
    );
}

#[test]
fn successful_unlock_updates_security_on_the_same_handle() {
    rt::reset();
    let current = json!({"encryption":{"state":"encrypted","requiresPassword":true},"permissions":{"known":true,"openedAs":"owner","canUpgradeToOwner":false},"access":{"required":false,"reasons":[]}});
    let fake = FakeEngine::new(
        json!({"documents":{"a":{"pages":[],"security":{"passwordPrompt":{"state":"required","incorrect":true}},"responses":[{"path":"security.unlock","value":{"security":current}}]}}}),
    );
    let doc = AsEngine(fake).open(json!({"id":"a"}), None).peek().unwrap().unwrap();
    let held = doc.security();
    assert_eq!(held.password_prompt()["state"], "required");
    held.unlock(json!({"password":"ok"}));
    assert_eq!(held.current(), current);
    assert_eq!(doc.security().password_prompt(), json!({"state":"none"}));
}
