#![cfg(feature = "uniffi")]
use epdf_ffi::KernelThread;
use epdf_plugin_annotation::{ANNOTATION, annotation_plugin, ffi::AnnotationHandle};
use futures::executor::block_on;
use plugin_conformance::test_kernel::{TestKernel, one_document};
use serde_json::{Value, json};

#[test]
fn annotation_settings_keep_registered_tool_defaults_and_reset() {
    let thread = KernelThread::spawn(|_| {
        TestKernel::open(
            vec![
                epdf_plugin_interaction::interaction_plugin(Default::default()),
                annotation_plugin(Some(json!({"tools":{"square":{"defaults":{"color":"red"}}}}))),
            ],
            one_document(),
        )
    })
    .unwrap();
    let post = thread.post();
    let handle = block_on(
        thread.call(move |k| AnnotationHandle::new(k.capability(&ANNOTATION).unwrap(), post)),
    )
    .unwrap();
    let settings = block_on(handle.settings()).unwrap();
    let original: Value = serde_json::from_str(&settings).unwrap();
    assert_eq!(original["tools"]["square"]["defaults"]["color"], "red");
    block_on(
        handle
            .update_settings(json!({"tools":{"square":{"defaults":{"color":"blue"}}}}).to_string()),
    )
    .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&block_on(handle.settings()).unwrap()).unwrap()["tools"]["square"]
            ["defaults"]["color"],
        "blue"
    );
    assert!(block_on(handle.update_settings("[]".into())).is_err());
    block_on(handle.reset_settings()).unwrap();
    assert_eq!(block_on(handle.settings()).unwrap(), settings);
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    assert!(block_on(handle.settings()).is_err());
    assert!(block_on(handle.update_settings("{}".into())).is_err());
    assert!(block_on(handle.reset_settings()).is_err());
}

#[test]
fn resetting_settings_keeps_runtime_tool_and_behavior_registrations() {
    struct RuntimeBehavior;
    impl epdf_plugin_annotation::host_contract::Behavior for RuntimeBehavior {
        fn id(&self) -> String {
            "runtime-behavior".into()
        }
        fn matches(&self, _: &epdf_plugin_annotation::js::Object) -> bool {
            true
        }
        fn engaged(&self, _: &epdf_plugin_annotation::js::Object) -> bool {
            true
        }
    }
    let thread = KernelThread::spawn(|_| {
        TestKernel::open(
            vec![
                epdf_plugin_interaction::interaction_plugin(Default::default()),
                annotation_plugin(None),
            ],
            one_document(),
        )
    })
    .unwrap();
    let post = thread.post();
    let handle = block_on(thread.call(move |k| {
        let cap = k.capability(&ANNOTATION).unwrap();
        cap.tools_register(json!({"id":"runtime-tool","extends":"square"})).unwrap();
        cap.register_behavior(std::rc::Rc::new(RuntimeBehavior));
        AnnotationHandle::new(cap, post)
    }))
    .unwrap();
    block_on(handle.update_settings("{\"snap\":{\"enabled\":false}}".into())).unwrap();
    block_on(handle.reset_settings()).unwrap();
    assert!(block_on(handle.tools().tool("runtime-tool".into())).unwrap().is_some());
    block_on(thread.call(|k| {
        let cap = k.capability(&ANNOTATION).unwrap();
        assert_eq!(cap.get_behavior_for(&Default::default()).unwrap().id(), "runtime-behavior");
    }))
    .unwrap();
}
