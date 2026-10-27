#![cfg(feature = "uniffi")]

use std::collections::HashMap;

use epdf_ffi::ViewerError;
use epdf_plugin_i18n::Locale;
use epdf_plugin_i18n::ffi::{LocalePack, TextDirection, locale_pack_from_json};

fn code(error: &ViewerError) -> Option<&str> {
    match error {
        ViewerError::Kernel { code, .. } => code.as_deref(),
        ViewerError::Closed => None,
    }
}

fn pack(code: &str, entries: &[(&str, &str)]) -> LocalePack {
    LocalePack {
        code: code.into(),
        name: code.to_uppercase(),
        dir: None,
        translations: entries.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
    }
}

#[test]
fn a_locale_json_file_flattens_to_dotted_keys() {
    let pack = locale_pack_from_json(
        r#"{"code":"es","name":"Español","direction":"ltr",
            "translations":{"zoom":{"in":"Acercar","out":"Alejar"},"pages":{"one":"{count} página","other":"{count} páginas"}}}"#
            .into(),
    )
    .unwrap();
    assert_eq!(pack.code, "es");
    assert_eq!(pack.dir, Some(TextDirection::Ltr));
    let expected: HashMap<String, String> = [
        ("zoom.in", "Acercar"),
        ("zoom.out", "Alejar"),
        ("pages.one", "{count} página"),
        ("pages.other", "{count} páginas"),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), v.into()))
    .collect();
    assert_eq!(pack.translations, expected);
}

#[test]
fn json_that_is_not_a_locale_is_invalid_input() {
    let error = locale_pack_from_json(r#"{"code":"es"}"#.into()).unwrap_err();
    assert_eq!(code(&error), Some("invalid-input"), "{error:?}");
}

#[test]
fn a_flat_pack_rebuilds_the_tree_the_plugin_reads() {
    let locale =
        Locale::try_from(pack("es", &[("zoom.in", "Acercar"), ("title", "Visor")])).unwrap();
    let json = serde_json::to_value(&locale.translations).unwrap();
    assert_eq!(json, serde_json::json!({ "title": "Visor", "zoom": { "in": "Acercar" } }));
}

#[test]
fn a_key_that_is_both_leaf_and_branch_is_refused() {
    let error = Locale::try_from(pack("es", &[("a", "x"), ("a.b", "y")])).unwrap_err();
    assert_eq!(error.code(), Some("invalid-input"), "{error:?}");
    assert!(error.message.contains("\"a"), "{error:?}");
}

#[test]
fn an_empty_key_segment_is_refused() {
    let error = Locale::try_from(pack("es", &[("zoom..in", "x")])).unwrap_err();
    assert_eq!(error.code(), Some("invalid-input"), "{error:?}");
}

use std::sync::{Arc, Mutex};

use epdf_ffi::{HostFailure, KernelThread};
use epdf_plugin_i18n::ffi::{
    I18nConfig, I18nEvent, I18nEventListener, I18nHandle, I18nParam, LocaleLoadCompletion,
    LocaleLoader,
};
use epdf_plugin_i18n::{I18N, i18n_plugin};
use futures::executor::block_on;
use plugin_conformance::test_kernel::{TestKernel, one_document};

#[derive(Default)]
struct Events(Mutex<Vec<I18nEvent>>);

impl I18nEventListener for Events {
    fn on_event(&self, event: I18nEvent) {
        self.0.lock().unwrap().push(event);
    }
}

/// What a test loader does with a request.
#[derive(Clone, Copy)]
enum Answer {
    Pack,
    Fail,
    Cancelled,
    Drop,
    Twice,
    Hold,
}

struct Loader {
    answer: Answer,
    asked: Mutex<Vec<String>>,
    held: Mutex<Vec<Arc<LocaleLoadCompletion>>>,
}

impl Loader {
    fn new(answer: Answer) -> Arc<Self> {
        Arc::new(Loader { answer, asked: Mutex::default(), held: Mutex::default() })
    }
}

impl LocaleLoader for Loader {
    fn load(&self, code: String, completion: Arc<LocaleLoadCompletion>) {
        self.asked.lock().unwrap().push(code.clone());
        match self.answer {
            Answer::Pack => completion.finish(pack(&code, &[("hello", "Hola")])),
            Answer::Fail => completion.fail("offline".into()),
            Answer::Cancelled => completion.fail_with(HostFailure {
                name: "AbortError".into(),
                code: Some("operation-cancelled".into()),
                detail: "operation cancelled".into(),
            }),
            Answer::Drop => drop(completion),
            Answer::Twice => {
                completion.finish(pack(&code, &[("hello", "Hola")]));
                completion.finish(pack(&code, &[("hello", "Second")]));
            }
            Answer::Hold => self.held.lock().unwrap().push(completion),
        }
    }
}

fn config(loader: Option<Arc<Loader>>) -> I18nConfig {
    I18nConfig {
        locale: Some("en".into()),
        fallback_locale: None,
        locales: vec![pack(
            "en",
            &[
                ("hello", "Hello"),
                ("greet", "Hi {name}"),
                ("pages.one", "{count} page"),
                ("pages.other", "{count} pages"),
            ],
        )],
        lazy_locales: if loader.is_some() { vec!["es".into()] } else { vec![] },
        loader: loader.map(|l| l as Arc<dyn LocaleLoader>),
    }
}

fn handle(config: I18nConfig) -> (KernelThread<TestKernel>, Arc<I18nHandle>) {
    let thread = KernelThread::spawn(move |post| {
        let config = config.into_plugin_config(post)?;
        TestKernel::open(vec![i18n_plugin(config)], one_document())
    })
    .unwrap();
    let post = thread.post();
    let handle =
        block_on(thread.call(move |k| I18nHandle::new(k.capability(&I18N).unwrap(), post)))
            .unwrap();
    (thread, handle)
}

#[test]
fn t_interpolates_and_picks_plural_forms() {
    let (_thread, i18n) = handle(config(None));
    let name = [("name".to_string(), I18nParam::Text { value: "Ana".into() })].into();
    assert_eq!(block_on(i18n.t("greet".into(), name, None)).unwrap(), "Hi Ana");
    let one = [("count".to_string(), I18nParam::Number { value: 1.0 })].into();
    assert_eq!(block_on(i18n.t("pages".into(), one, None)).unwrap(), "1 page");
    let many = [("count".to_string(), I18nParam::Number { value: 3.0 })].into();
    assert_eq!(block_on(i18n.t("pages".into(), many, None)).unwrap(), "3 pages");
    let missing = block_on(i18n.t("nope".into(), HashMap::new(), Some("Fallback".into())));
    assert_eq!(missing.unwrap(), "Fallback");
    assert!(block_on(i18n.has_key("hello".into())).unwrap());
    assert!(!block_on(i18n.has_key("nope".into())).unwrap());
}

#[test]
fn a_lazy_switch_loads_the_pack_and_announces_it() {
    let loader = Loader::new(Answer::Pack);
    let (_thread, i18n) = handle(config(Some(loader.clone())));
    let events = Arc::new(Events::default());
    let _sub = i18n.subscribe(events.clone());
    let listed = block_on(i18n.locales()).unwrap();
    assert_eq!(
        listed.iter().map(|l| (l.code.as_str(), l.loaded)).collect::<Vec<_>>(),
        [("en", true), ("es", false)]
    );
    block_on(i18n.set_locale("es".into())).unwrap();
    assert_eq!(*loader.asked.lock().unwrap(), ["es"]);
    assert_eq!(block_on(i18n.locale()).unwrap(), "es");
    assert_eq!(block_on(i18n.t("hello".into(), HashMap::new(), None)).unwrap(), "Hola");
    let seen = events.0.lock().unwrap();
    assert!(
        matches!(seen.last(), Some(I18nEvent::LocaleChanged { event }) if event.locale == "es" && event.previous_locale == "en"),
        "{seen:?}"
    );
}

#[test]
fn a_failed_load_rejects_and_reports() {
    let (_thread, i18n) = handle(config(Some(Loader::new(Answer::Fail))));
    let events = Arc::new(Events::default());
    let _sub = i18n.subscribe(events.clone());
    let error = block_on(i18n.set_locale("es".into())).unwrap_err();
    assert_eq!(code(&error), Some("operation-failed"), "{error:?}");
    assert_eq!(block_on(i18n.locale()).unwrap(), "en");
    let seen = events.0.lock().unwrap();
    assert!(
        matches!(seen.last(), Some(I18nEvent::LocaleLoadFailed { event }) if event.locale == "es" && event.error.message.contains("offline")),
        "{seen:?}"
    );
}

#[test]
fn a_failure_with_a_code_keeps_it_in_the_load_failed_event() {
    let (_thread, i18n) = handle(config(Some(Loader::new(Answer::Cancelled))));
    let events = Arc::new(Events::default());
    let _sub = i18n.subscribe(events.clone());
    // The switch itself rejects `operation-failed` with the cause in its
    // details, as `sync/loaders.ts` does; the event carries the cause.
    let error = block_on(i18n.set_locale("es".into())).unwrap_err();
    assert_eq!(code(&error), Some("operation-failed"), "{error:?}");
    let seen = events.0.lock().unwrap();
    assert!(
        matches!(seen.last(), Some(I18nEvent::LocaleLoadFailed { event }) if event.error.code == "operation-cancelled"),
        "{seen:?}"
    );
}

#[test]
fn a_dropped_completion_fails_the_switch() {
    let (_thread, i18n) = handle(config(Some(Loader::new(Answer::Drop))));
    let error = block_on(i18n.set_locale("es".into())).unwrap_err();
    assert_eq!(code(&error), Some("operation-failed"), "{error:?}");
    assert_eq!(block_on(i18n.loading_locale()).unwrap(), None);
}

#[test]
fn a_second_answer_is_ignored() {
    let (_thread, i18n) = handle(config(Some(Loader::new(Answer::Twice))));
    block_on(i18n.set_locale("es".into())).unwrap();
    assert_eq!(block_on(i18n.t("hello".into(), HashMap::new(), None)).unwrap(), "Hola");
}

#[test]
fn a_newer_switch_cancels_the_pending_one() {
    let loader = Loader::new(Answer::Hold);
    let (thread, i18n) = handle(config(Some(loader.clone())));
    // A Rust future runs only when polled, so the first switch waits on its
    // own thread while this one supersedes it.
    let first = {
        let i18n = i18n.clone();
        std::thread::spawn(move || block_on(i18n.set_locale("es".into())))
    };
    while loader.held.lock().unwrap().is_empty() {
        block_on(thread.call(|_| ())).unwrap();
    }
    assert_eq!(block_on(i18n.loading_locale()).unwrap(), Some("es".into()));
    block_on(i18n.set_locale("en".into())).unwrap();
    let error = first.join().unwrap().unwrap_err();
    assert_eq!(code(&error), Some("operation-cancelled"), "{error:?}");
    // The held answer arrives late and changes nothing.
    let late = loader.held.lock().unwrap().pop().unwrap();
    late.finish(pack("es", &[("hello", "Hola")]));
    block_on(thread.call(|_| ())).unwrap();
    assert_eq!(block_on(i18n.locale()).unwrap(), "en");
}

#[test]
fn an_unknown_locale_is_not_found() {
    let (_thread, i18n) = handle(config(None));
    let error = block_on(i18n.set_locale("fr".into())).unwrap_err();
    assert_eq!(code(&error), Some("not-found"), "{error:?}");
}

#[test]
fn registered_packs_and_added_keys_are_read_and_removable() {
    let (_thread, i18n) = handle(config(None));
    let registration = block_on(i18n.register_locale(pack("de", &[("hello", "Hallo")]))).unwrap();
    block_on(
        i18n.add_translations("de".into(), [("bye".to_string(), "Tschüss".to_string())].into()),
    )
    .unwrap();
    block_on(i18n.set_locale("de".into())).unwrap();
    assert_eq!(block_on(i18n.t("bye".into(), HashMap::new(), None)).unwrap(), "Tschüss");
    block_on(i18n.set_locale("en".into())).unwrap();
    registration.unregister();
    let error = block_on(i18n.set_locale("de".into())).unwrap_err();
    assert_eq!(code(&error), Some("not-found"), "{error:?}");
    let error = block_on(i18n.add_translations("de".into(), HashMap::new())).unwrap_err();
    assert_eq!(code(&error), Some("not-found"), "{error:?}");
}

#[test]
fn a_cancelled_subscription_hears_nothing_more() {
    let (thread, i18n) = handle(config(None));
    let events = Arc::new(Events::default());
    let subscription = i18n.subscribe(events.clone());
    block_on(i18n.register_locale(pack("de", &[]))).unwrap();
    block_on(i18n.set_locale("de".into())).unwrap();
    assert_eq!(events.0.lock().unwrap().len(), 1);
    subscription.cancel();
    block_on(thread.call(|_| ())).unwrap();
    block_on(i18n.set_locale("en".into())).unwrap();
    assert_eq!(events.0.lock().unwrap().len(), 1);
}

#[test]
fn a_call_after_the_kernel_thread_stops_is_closed() {
    let (thread, i18n) = handle(config(None));
    drop(thread);
    let error = block_on(i18n.locale()).unwrap_err();
    assert!(matches!(error, ViewerError::Closed), "{error:?}");
}

#[test]
fn configs_compare_their_loader_by_presence() {
    // A foreign loader arrives in a fresh `Arc` each time the config
    // crosses, so identity can't be what equality checks.
    assert_eq!(config(Some(Loader::new(Answer::Pack))), config(Some(Loader::new(Answer::Fail))));
    assert_eq!(config(None), config(None));
    let mut lazy = config(None);
    lazy.loader = Some(Loader::new(Answer::Pack));
    assert_ne!(lazy, config(None));
}
