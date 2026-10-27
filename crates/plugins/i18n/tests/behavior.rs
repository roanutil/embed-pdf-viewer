//! The typed API beyond what the vectors print, against
//! `packages/plugin/i18n/src/{controller,model,translate,negotiate}.ts`,
//! `sync/loaders.ts` and `strings/index.ts`: the missing-key warning, the
//! fallback key and EmbedPDF's own strings, script-aware negotiation, the
//! workspace settings, loading the same code once, superseded and cancelled
//! waiters, a pack that lands late or under another code, references that
//! stay the same until their inputs change, close, and the turns each
//! switch settles on.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use epdf_kernel::error::code;
use epdf_kernel::rt::Resolver;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{Error, Kernel, KernelOptions, Promise, rt};
use epdf_plugin_i18n::*;
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use serde_json::json;

fn pack(code: &str, name: &str, translations: serde_json::Value) -> Locale {
    Locale {
        code: code.into(),
        name: name.into(),
        direction: None,
        translations: serde_json::from_value(translations).expect("a dictionary"),
    }
}

fn en() -> Locale {
    pack(
        "en",
        "English",
        json!({ "commands": { "save": "Save" }, "pages": { "one": "{count} page", "other": "{count} pages" } }),
    )
}

fn es() -> Locale {
    pack("es", "Español", json!({ "commands": { "save": "Guardar" } }))
}

fn he() -> Locale {
    Locale {
        direction: Some(TextDirection::Rtl),
        ..pack("he", "עברית", json!({ "commands": { "save": "שמור" } }))
    }
}

fn ar() -> Locale {
    Locale {
        direction: Some(TextDirection::Rtl),
        ..pack("ar", "العربية", json!({ "commands": { "save": "حفظ" } }))
    }
}

/// A loader whose answer the test gives later, counting its calls.
#[derive(Clone, Default)]
struct Deferred {
    calls: Rc<Cell<u32>>,
    resolvers: Rc<RefCell<Vec<Resolver<Locale>>>>,
}

impl Deferred {
    fn loader(&self) -> LocaleLoader {
        let this = self.clone();
        Rc::new(move || {
            this.calls.set(this.calls.get() + 1);
            let (promise, resolver) = Promise::pending();
            this.resolvers.borrow_mut().push(resolver);
            Ok(promise)
        })
    }

    fn resolve(&self, locale: Locale) {
        let resolver = self.resolvers.borrow_mut().remove(0);
        resolver.resolve(locale);
        rt::run_until_stalled();
    }

    fn reject(&self, message: &str) {
        let resolver = self.resolvers.borrow_mut().remove(0);
        resolver.reject(Error::new(message));
        rt::run_until_stalled();
    }
}

/// A loader that answers at once.
fn ready(locale: Locale) -> LocaleLoader {
    Rc::new(move || Ok(Promise::resolved(locale.clone())))
}

fn loaders(entries: Vec<(&str, LocaleLoader)>) -> JsRecord<LocaleLoader> {
    entries.into_iter().map(|(code, loader)| (code.to_string(), loader)).collect()
}

struct Rig {
    kernel: Kernel,
    i18n: I18nCapability,
    warnings: Rc<RefCell<Vec<String>>>,
    /// The turn counter a test binds, and the turn of each warning.
    clock: Rc<RefCell<Option<Rc<Cell<u32>>>>>,
    warned_on: Rc<RefCell<Vec<u32>>>,
    reports: Rc<RefCell<Vec<String>>>,
    changed: Rc<RefCell<Vec<(String, String)>>>,
}

impl Rig {
    fn new(config: I18nConfig) -> Rig {
        rt::reset();
        let warnings: Rc<RefCell<Vec<String>>> = Rc::default();
        let reports: Rc<RefCell<Vec<String>>> = Rc::default();
        let clock: Rc<RefCell<Option<Rc<Cell<u32>>>>> = Rc::default();
        let warned_on: Rc<RefCell<Vec<u32>>> = Rc::default();
        let (warned, heard) = (warnings.clone(), reports.clone());
        let (at, turns) = (clock.clone(), warned_on.clone());
        let fake = FakeEngine::new(json!({ "documents": {} }));
        let kernel = Kernel::new(
            Rc::new(AsEngine(fake)),
            vec![i18n_plugin_warning_to(
                config,
                Rc::new(move |message: &str| {
                    warned.borrow_mut().push(message.to_string());
                    if let Some(turn) = &*at.borrow() {
                        turns.borrow_mut().push(turn.get());
                    }
                }),
            )],
            KernelOptions {
                report: Some(Rc::new(move |error: &Error| {
                    heard.borrow_mut().push(error.message.clone())
                })),
                ..KernelOptions::default()
            },
        )
        .expect("kernel");
        kernel.start();
        rt::run_until_stalled();
        let i18n = kernel.capability(&I18N, None).expect("i18n");
        let changed: Rc<RefCell<Vec<(String, String)>>> = Rc::default();
        let log = changed.clone();
        i18n.on_locale_changed(move |event| {
            log.borrow_mut().push((event.previous_locale.clone(), event.locale.clone()));
            Ok(())
        });
        Rig { kernel, i18n, warnings, clock, warned_on, reports, changed }
    }

    fn with(loaders: JsRecord<LocaleLoader>) -> Rig {
        Rig::new(I18nConfig {
            locales: Some(vec![en(), es()]),
            loaders: Some(loaders),
            ..I18nConfig::default()
        })
    }

    fn switch(&self, code: &str) -> Promise<()> {
        let promise = self.i18n.set_locale(code, OperationOptions::default());
        rt::run_until_stalled();
        promise
    }

    fn listed(&self) -> Vec<(String, bool)> {
        self.i18n.list_locales().iter().map(|info| (info.code.clone(), info.loaded)).collect()
    }
}

fn rejection(promise: &Promise<()>) -> Option<(String, String)> {
    match promise.peek()? {
        Ok(()) => None,
        Err(error) => Some((error.code().unwrap_or_default().to_string(), error.message.clone())),
    }
}

fn options(fallback_key: Option<&str>, fallback: Option<&str>) -> TranslateOptions {
    TranslateOptions {
        fallback_key: fallback_key.map(str::to_string),
        fallback: fallback.map(str::to_string),
        ..TranslateOptions::default()
    }
}

#[test]
fn a_missing_key_warns_once_and_never_through_has_key_or_a_fallback() {
    let rig = Rig::with(JsRecord::new());
    assert_eq!(rig.i18n.t("nope", None), "nope");
    assert_eq!(rig.i18n.t("nope", None), "nope");
    assert!(!rig.i18n.has_key("nope"));
    assert_eq!(rig.i18n.t("nope", Some(&options(None, Some("Shown")))), "Shown");
    // The key and its fallback key are one miss of their own.
    assert_eq!(rig.i18n.t("nope", Some(&options(Some("other"), None))), "nope");
    assert_eq!(rig.i18n.t("nope", Some(&options(Some("other"), None))), "nope");
    // The warning names the locale current at the first miss.
    rig.switch("es");
    assert_eq!(rig.i18n.t("nope", None), "nope");
    assert_eq!(
        *rig.warnings.borrow(),
        [
            "[i18n] missing translation \"nope\" (locale: en)",
            "[i18n] missing translation \"nope\" or \"other\" (locale: en)",
        ]
    );
}

#[test]
fn a_fallback_key_is_read_in_each_language_before_the_fallback_language() {
    let rig = Rig::new(I18nConfig {
        locale: Some(Some("es".into())),
        locales: Some(vec![en(), es(), pack("xx", "X", json!({}))]),
        ..I18nConfig::default()
    });
    let save = options(Some("commands.save"), None);
    // The current language's `commands.save` before the fallback language's.
    assert_eq!(rig.i18n.t("review.accept", Some(&save)), "Guardar");
    rig.switch("xx");
    assert_eq!(rig.i18n.t("review.accept", Some(&save)), "Save");
    // A key the current language has wins over the fallback key.
    rig.i18n
        .add_translations("xx", serde_json::from_value(json!({ "review.accept": "Ok" })).unwrap())
        .expect("xx is loaded");
    assert_eq!(rig.i18n.t("review.accept", Some(&save)), "Ok");
    assert!(rig.warnings.borrow().is_empty());
}

#[test]
fn embedpdfs_own_strings_sit_under_the_apps_in_the_matched_language() {
    let rig = Rig::new(I18nConfig {
        locale: Some(Some("de-AT".into())),
        locales: Some(vec![en(), pack("de-AT", "Deutsch", json!({}))]),
        ..I18nConfig::default()
    });
    // `de-AT` reads EmbedPDF's German.
    assert_eq!(rig.i18n.t("commands.zoom.in", None), "Vergrößern");
    // The app's string replaces EmbedPDF's.
    rig.i18n
        .add_translations(
            "de-AT",
            serde_json::from_value(json!({ "commands.zoom.in": "Größer" })).unwrap(),
        )
        .expect("de-AT is loaded");
    assert_eq!(rig.i18n.t("commands.zoom.in", None), "Größer");
    // A language EmbedPDF has no strings in falls back to the fallback
    // language's own strings.
    let rig = Rig::new(I18nConfig {
        locale: Some(Some("ko".into())),
        locales: Some(vec![pack("ko", "한국어", json!({}))]),
        ..I18nConfig::default()
    });
    assert_eq!(rig.i18n.t("commands.zoom.in", None), "Zoom in");
}

#[test]
fn negotiation_keeps_to_the_script_a_language_is_written_in() {
    // `zh-TW` and `zh-HK` are Traditional: Simplified Chinese is not theirs.
    assert_eq!(negotiate_locale(&["zh-CN", "en"], &["zh-TW"]), None);
    assert_eq!(negotiate_locale(&["zh-CN", "zh-Hant"], &["zh-HK"]), Some("zh-Hant".into()));
    assert_eq!(negotiate_locale(&["zh-Hans", "zh-Hant"], &["zh"]), Some("zh-Hans".into()));
    // Serbian is Cyrillic unless it says otherwise; Montenegro is Latin.
    assert_eq!(negotiate_locale(&["sr-Latn"], &["sr"]), None);
    assert_eq!(negotiate_locale(&["sr-Latn"], &["sr-ME"]), Some("sr-Latn".into()));
    // A tag whose script can't be told matches on its language alone.
    assert_eq!(negotiate_locale(&["en"], &["en-x-a"]), Some("en".into()));
    // EmbedPDF's own strings follow the same rules.
    assert!(embedpdf_strings_for("zh-TW").is_none());
    assert!(embedpdf_strings_for("zh").is_some());
    assert_eq!(embedpdf_strings_for("nl-BE"), embedpdf_strings_for("nl"));
    assert!(embedpdf_strings_for("nl").is_some());
}

#[test]
fn the_same_code_loads_once_and_a_second_switch_replaces_the_first_waiter() {
    let deferred = Deferred::default();
    let rig = Rig::with(loaders(vec![("ar", deferred.loader())]));
    let first = rig.switch("ar");
    let second = rig.switch("ar");
    assert_eq!(deferred.calls.get(), 1);
    assert_eq!(
        rejection(&first),
        Some((code::OPERATION_CANCELLED.into(), "switching to 'ar' was replaced".into()))
    );
    assert!(second.peek().is_none());
    deferred.resolve(ar());
    assert!(matches!(second.peek(), Some(Ok(()))));
    assert_eq!(rig.i18n.get_locale(), "ar");
    assert_eq!(rig.i18n.get_direction(), TextDirection::Rtl);
    // Loaded now: a plain switch, no loader call.
    rig.switch("en");
    rig.switch("ar");
    assert_eq!(deferred.calls.get(), 1);
    assert_eq!(
        *rig.changed.borrow(),
        [("en".into(), "ar".into()), ("ar".into(), "en".into()), ("en".into(), "ar".into())]
    );
}

#[test]
fn a_pack_that_lands_after_a_newer_switch_registers_without_switching() {
    let (ar_loader, he_loader) = (Deferred::default(), Deferred::default());
    let rig = Rig::with(loaders(vec![("ar", ar_loader.loader()), ("he", he_loader.loader())]));
    let to_ar = rig.switch("ar");
    let to_he = rig.switch("he");
    assert_eq!(ar_loader.calls.get(), 1);
    assert_eq!(he_loader.calls.get(), 1);
    assert_eq!(rejection(&to_ar).map(|(code, _)| code), Some(code::OPERATION_CANCELLED.into()));
    ar_loader.resolve(ar());
    assert_eq!(rig.i18n.get_locale(), "en");
    assert_eq!(rig.i18n.get_loading_locale().as_deref(), Some("he"));
    assert_eq!(
        rig.listed(),
        [("en".into(), true), ("es".into(), true), ("ar".into(), true), ("he".into(), false)]
    );
    assert!(rig.changed.borrow().is_empty());
    he_loader.resolve(he());
    assert!(matches!(to_he.peek(), Some(Ok(()))));
    assert_eq!(rig.i18n.get_locale(), "he");
    // A registered switch while loading ends the load; the pack that lands
    // later only registers.
    let late = Deferred::default();
    let rig = Rig::with(loaders(vec![("ar", late.loader())]));
    let to_ar = rig.switch("ar");
    let to_es = rig.switch("es");
    assert_eq!(rejection(&to_ar).map(|(code, _)| code), Some(code::OPERATION_CANCELLED.into()));
    assert!(matches!(to_es.peek(), Some(Ok(()))));
    assert_eq!(rig.i18n.get_loading_locale(), None);
    late.resolve(ar());
    assert_eq!(rig.i18n.get_locale(), "es");
    assert_eq!(rig.listed().last(), Some(&("ar".to_string(), true)));
    assert_eq!(*rig.changed.borrow(), [("en".into(), "es".into())]);
}

#[test]
fn a_pack_loaded_under_another_code_registers_and_shows_under_its_own() {
    let deferred = Deferred::default();
    let rig = Rig::with(loaders(vec![("ar", deferred.loader())]));
    let switched = rig.switch("ar");
    deferred.resolve(Locale { code: "ar-EG".into(), ..ar() });
    assert!(matches!(switched.peek(), Some(Ok(()))));
    assert_eq!(rig.i18n.get_locale(), "ar-EG");
    assert_eq!(rig.i18n.get_loading_locale(), None);
    // `ar` is still only in `loaders`: switching to it loads it again.
    assert_eq!(
        rig.listed(),
        [("en".into(), true), ("es".into(), true), ("ar-EG".into(), true), ("ar".into(), false)]
    );
    let again = rig.switch("ar");
    assert_eq!(deferred.calls.get(), 2);
    deferred.resolve(ar());
    assert!(matches!(again.peek(), Some(Ok(()))));
    assert_eq!(rig.i18n.get_locale(), "ar");
    assert_eq!(
        *rig.changed.borrow(),
        [("en".into(), "ar-EG".into()), ("ar-EG".into(), "ar".into())]
    );
}

#[test]
fn a_failed_load_rejects_with_the_failure_and_can_be_retried() {
    let deferred = Deferred::default();
    let rig = Rig::with(loaders(vec![("ar", deferred.loader())]));
    let failures: Rc<RefCell<Vec<serde_json::Value>>> = Rc::default();
    let log = failures.clone();
    rig.i18n.on_locale_load_failed(move |event| {
        log.borrow_mut().push(json!({ "locale": event.locale, "error": event.error }));
        Ok(())
    });
    let switched = rig.switch("ar");
    deferred.reject("offline");
    let error = match switched.peek() {
        Some(Err(error)) => error,
        other => panic!("rejected, got {other:?}"),
    };
    assert_eq!(error.code(), Some(code::OPERATION_FAILED));
    assert_eq!(error.message, "locale 'ar' failed to load");
    let info = json!({ "code": "operation-failed", "message": "offline", "capability": "i18n", "permission": null });
    assert_eq!(error.member("details"), Some(&info));
    assert_eq!(*failures.borrow(), [json!({ "locale": "ar", "error": info })]);
    assert_eq!(rig.i18n.get_loading_locale(), None);
    let retried = rig.switch("ar");
    assert_eq!(deferred.calls.get(), 2);
    deferred.resolve(ar());
    assert!(matches!(retried.peek(), Some(Ok(()))));
}

#[test]
fn cancelling_a_switch_stops_the_waiting_not_the_fetch() {
    let deferred = Deferred::default();
    let rig = Rig::with(loaders(vec![("ar", deferred.loader())]));
    let signal = AbortSignal::new();
    let switched = rig.i18n.set_locale("ar", OperationOptions { signal: Some(signal.clone()) });
    rt::run_until_stalled();
    assert_eq!(rig.i18n.get_loading_locale().as_deref(), Some("ar"));
    signal.abort(Error::new("stop"));
    rt::run_until_stalled();
    assert_eq!(rejection(&switched).map(|(code, _)| code), Some(code::OPERATION_CANCELLED.into()));
    assert_eq!(rig.i18n.get_loading_locale(), None);
    // The fetch was not cancelled: the pack still registers, and doesn't
    // become current.
    deferred.resolve(ar());
    assert_eq!(rig.i18n.get_locale(), "en");
    assert!(rig.listed().contains(&("ar".to_string(), true)));
    // A signal that already fired refuses before anything loads.
    let fresh = Deferred::default();
    let rig = Rig::with(loaders(vec![("he", fresh.loader())]));
    let signal = AbortSignal::new();
    signal.abort(Error::new("stop"));
    let refused = rig.i18n.set_locale("he", OperationOptions { signal: Some(signal) });
    rt::run_until_stalled();
    assert_eq!(
        rejection(&refused),
        Some((code::OPERATION_CANCELLED.into(), "operation cancelled".into()))
    );
    assert_eq!(fresh.calls.get(), 0);
    assert_eq!(rig.i18n.get_loading_locale(), None);
}

#[test]
fn an_unknown_code_is_not_found_for_switching_and_adding_strings() {
    let rig = Rig::with(loaders(vec![("he", ready(he()))]));
    let message = "unknown locale 'xx': not loaded and not in loaders";
    assert_eq!(rejection(&rig.switch("xx")), Some((code::NOT_FOUND.into(), message.into())));
    let refused = rig.i18n.add_translations("xx", JsRecord::new()).expect_err("unknown");
    assert_eq!((refused.code(), refused.message.as_str()), (Some(code::NOT_FOUND), message));
    // A code in `loaders` takes strings before it loads, on top of its own.
    rig.i18n
        .add_translations(
            "he",
            serde_json::from_value(json!({ "commands.save": "Later" })).unwrap(),
        )
        .expect("he has a loader");
    rig.switch("he");
    assert_eq!(rig.i18n.t("commands.save", None), "Later");
}

#[test]
fn the_settings_are_the_workspaces_and_changes_follow_through() {
    let rig = Rig::with(loaders(vec![("he", ready(he()))]));
    let events: Rc<RefCell<Vec<Vec<String>>>> = Rc::default();
    let log = events.clone();
    rig.i18n.on_settings_changed(move |event| {
        log.borrow_mut().push(event.changed.clone());
        Ok(())
    });
    let settings = rig.i18n.get_settings();
    assert!(Rc::ptr_eq(&settings, &rig.i18n.get_settings()));
    assert_eq!(settings.fallback_locale, "en");
    assert_eq!(settings.locales.iter().map(|l| l.code.as_str()).collect::<Vec<_>>(), ["en", "es"]);

    // New languages register; ones left out unregister.
    rig.i18n
        .update_settings(&I18nConfig { locales: Some(vec![en(), ar()]), ..I18nConfig::default() });
    rt::run_until_stalled();
    assert_eq!(rig.listed(), [("en".into(), true), ("ar".into(), true), ("he".into(), false)]);
    // A new `locale` switches to it, loading it first.
    rig.i18n
        .update_settings(&I18nConfig { locale: Some(Some("he".into())), ..I18nConfig::default() });
    rt::run_until_stalled();
    assert_eq!(rig.i18n.get_locale(), "he");
    // The fallback language applies to the next lookup.
    rig.i18n.update_settings(&I18nConfig {
        fallback_locale: Some("ar".into()),
        locale: Some(Some("en".into())),
        ..I18nConfig::default()
    });
    rt::run_until_stalled();
    rig.i18n
        .add_translations("ar", serde_json::from_value(json!({ "only": "عربي" })).unwrap())
        .unwrap();
    assert_eq!(rig.i18n.t("only", None), "عربي");
    // The same loader again changes nothing.
    let he_loader = rig.i18n.get_settings().loaders.get("he").cloned().expect("he");
    rig.i18n.update_settings(&I18nConfig {
        loaders: Some(loaders(vec![("he", he_loader)])),
        ..I18nConfig::default()
    });
    // A new loader is listed.
    rig.i18n.update_settings(&I18nConfig {
        loaders: Some(loaders(vec![("fr", ready(pack("fr", "Français", json!({}))))])),
        ..I18nConfig::default()
    });
    assert_eq!(rig.listed().last(), Some(&("fr".to_string(), false)));
    // Reset goes back to what the app registered.
    rig.i18n.reset_settings();
    rt::run_until_stalled();
    assert_eq!(rig.i18n.get_locale(), "en");
    assert_eq!(rig.i18n.get_settings().fallback_locale, "en");
    assert_eq!(
        *events.borrow(),
        [
            vec!["locales".to_string()],
            vec!["locale".to_string()],
            vec!["locale".to_string(), "fallbackLocale".to_string()],
            vec!["loaders".to_string()],
            vec![
                "locale".to_string(),
                "fallbackLocale".to_string(),
                "locales".to_string(),
                "loaders".to_string()
            ],
        ]
    );
    // A `locale` nothing knows warns, a turn after the change.
    rig.i18n
        .update_settings(&I18nConfig { locale: Some(Some("xx".into())), ..I18nConfig::default() });
    assert!(rig.warnings.borrow().is_empty());
    rt::run_until_stalled();
    assert_eq!(
        *rig.warnings.borrow(),
        ["[i18n] unknown locale 'xx': not loaded and not in loaders"]
    );
}

#[test]
fn readers_hold_the_same_reference_until_their_inputs_change() {
    let rig = Rig::with(loaders(vec![("he", ready(he()))]));
    let t = rig.i18n.get_translator();
    let listed = rig.i18n.list_locales();
    assert!(Rc::ptr_eq(&t, &rig.i18n.get_translator()));
    assert!(Rc::ptr_eq(&listed, &rig.i18n.list_locales()));
    assert_eq!(t("commands.save", None), "Save");
    // A load that changes nothing the readers read keeps them.
    let unchanged = rig.switch("en");
    assert!(matches!(unchanged.peek(), Some(Ok(()))));
    rig.i18n.update_settings(&I18nConfig { locale: Some(None), ..I18nConfig::default() });
    assert!(Rc::ptr_eq(&t, &rig.i18n.get_translator()));
    assert!(Rc::ptr_eq(&listed, &rig.i18n.list_locales()));
    // A new language: a new translator and a new list.
    rig.switch("es");
    let t_es = rig.i18n.get_translator();
    assert!(!Rc::ptr_eq(&t, &t_es));
    assert!(Rc::ptr_eq(&listed, &rig.i18n.list_locales()));
    assert_eq!(t_es("commands.save", None), "Guardar");
    // New strings: a new translator; a new loader: a new list.
    rig.i18n.add_translations("es", serde_json::from_value(json!({ "x": "y" })).unwrap()).unwrap();
    assert!(!Rc::ptr_eq(&t_es, &rig.i18n.get_translator()));
    rig.i18n.update_settings(&I18nConfig {
        loaders: Some(loaders(vec![("ar", ready(ar()))])),
        ..I18nConfig::default()
    });
    assert!(!Rc::ptr_eq(&listed, &rig.i18n.list_locales()));
}

#[test]
fn after_close_a_late_pack_is_dropped_and_reported_once() {
    let deferred = Deferred::default();
    let rig = Rig::with(loaders(vec![("ar", deferred.loader())]));
    let switched = rig.switch("ar");
    rig.kernel.destroy();
    rt::run_until_stalled();
    deferred.resolve(ar());
    // The state still reads `loading: ar`, so the waiter settles as the
    // TypeScript's does; both writes are refused, reported once.
    assert!(matches!(switched.peek(), Some(Ok(()))));
    let reports = rig.reports.borrow();
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert!(
        reports[0].starts_with("a state update after the instance closed (i18n#"),
        "{reports:?}"
    );
    assert!(rig.changed.borrow().is_empty());
    assert!(rig.kernel.capability(&I18N, None).is_err());
}

/// A counter that moves once per turn of the queue, as
/// `(async () => { for (;;) { await undefined; turn++ } })()` does.
fn turn_counter() -> Rc<Cell<u32>> {
    let turn = Rc::new(Cell::new(0));
    let count = turn.clone();
    rt::spawn(async move {
        for _ in 0..64 {
            rt::tick().await;
            count.set(count.get() + 1);
        }
    });
    turn
}

/// `promise.then(() => at = turn, () => at = turn)`: the turn its reaction
/// runs on, and whether it rejected.
fn reacts_on(promise: Promise<()>, turn: &Rc<Cell<u32>>) -> Rc<Cell<(u32, bool)>> {
    let at = Rc::new(Cell::new((u32::MAX, false)));
    let (seen, turn) = (at.clone(), turn.clone());
    rt::spawn(async move {
        let rejected = promise.await.is_err();
        seen.set((turn.get(), rejected));
    });
    at
}

/// The turn each `onLocaleChanged` / `onLocaleLoadFailed` fired on.
fn heard_on(rig: &Rig, turn: &Rc<Cell<u32>>) -> Rc<RefCell<Vec<(String, u32)>>> {
    let heard: Rc<RefCell<Vec<(String, u32)>>> = Rc::default();
    let (log, at) = (heard.clone(), turn.clone());
    rig.i18n.on_locale_changed(move |event| {
        log.borrow_mut().push((format!("changed {}", event.locale), at.get()));
        Ok(())
    });
    let (log, at) = (heard.clone(), turn.clone());
    rig.i18n.on_locale_load_failed(move |event| {
        log.borrow_mut().push((format!("failed {}", event.locale), at.get()));
        Ok(())
    });
    heard
}

/// The turns were measured on the TypeScript: the real kernel and i18n
/// plugin (`en` and `es` loaded) over the conformance fake engine, with the
/// counter above and `promise.then(() => at = turn, () => at = turn)`, run
/// under `tsx --conditions=development`. A switch to a loaded language
/// fires `onLocaleChanged` on 0 and settles on 1; an unknown code rejects
/// on 1. A loader that answers at once fires `onLocaleChanged` on 1 and the
/// switch settles on 2; one that rejects fires `onLocaleLoadFailed` on 1
/// and rejects on 2. A switch replaced by another (lazy or loaded) rejects
/// on 1, as does a switch whose signal fires while loading or had fired.
/// `updateSettings({ locale })` warns about a code nothing knows on 1, and
/// a lazy one shows on 1.
#[test]
fn switches_settle_on_the_typescripts_turns() {
    let measure = |loaders_of: Vec<(&str, LocaleLoader)>| {
        let rig = Rig::with(loaders(loaders_of));
        let turn = turn_counter();
        let heard = heard_on(&rig, &turn);
        (rig, turn, heard)
    };
    let heard_list = |heard: &Rc<RefCell<Vec<(String, u32)>>>| heard.borrow().clone();
    let none = OperationOptions::default;

    let (rig, turn, heard) = measure(vec![("he", ready(he()))]);
    let he_at = reacts_on(rig.i18n.set_locale("he", none()), &turn);
    rt::run_until_stalled();
    assert_eq!(he_at.get(), (2, false), "lazy, resolved");
    assert_eq!(heard_list(&heard), [("changed he".to_string(), 1)]);

    let failing: LocaleLoader = Rc::new(|| Ok(Promise::rejected(Error::new("offline"))));
    let (rig, turn, heard) = measure(vec![("he", failing)]);
    let he_at = reacts_on(rig.i18n.set_locale("he", none()), &turn);
    rt::run_until_stalled();
    assert_eq!(he_at.get(), (2, true), "lazy, rejected");
    assert_eq!(heard_list(&heard), [("failed he".to_string(), 1)]);

    let (rig, turn, heard) = measure(vec![]);
    let es_at = reacts_on(rig.i18n.set_locale("es", none()), &turn);
    let xx_at = reacts_on(rig.i18n.set_locale("xx", none()), &turn);
    rt::run_until_stalled();
    assert_eq!((es_at.get(), xx_at.get()), ((1, false), (1, true)), "loaded, unknown");
    assert_eq!(heard_list(&heard), [("changed es".to_string(), 0)]);

    let (ar_loader, he_loader) = (Deferred::default(), Deferred::default());
    let (rig, turn, heard) = measure(vec![("ar", ar_loader.loader()), ("he", he_loader.loader())]);
    let ar_at = reacts_on(rig.i18n.set_locale("ar", none()), &turn);
    let he_at = reacts_on(rig.i18n.set_locale("he", none()), &turn);
    he_loader.resolvers.borrow_mut().remove(0).resolve(he());
    rt::run_until_stalled();
    assert_eq!((ar_at.get(), he_at.get()), ((1, true), (2, false)), "replaced by a lazy switch");
    assert_eq!(heard_list(&heard), [("changed he".to_string(), 1)]);

    let ar_loader = Deferred::default();
    let (rig, turn, heard) = measure(vec![("ar", ar_loader.loader())]);
    let ar_at = reacts_on(rig.i18n.set_locale("ar", none()), &turn);
    let es_at = reacts_on(rig.i18n.set_locale("es", none()), &turn);
    rt::run_until_stalled();
    assert_eq!((ar_at.get(), es_at.get()), ((1, true), (1, false)), "replaced by a loaded switch");
    assert_eq!(heard_list(&heard), [("changed es".to_string(), 0)]);

    let ar_loader = Deferred::default();
    let (rig, turn, _) = measure(vec![("ar", ar_loader.loader())]);
    let signal = AbortSignal::new();
    let ar_at = reacts_on(
        rig.i18n.set_locale("ar", OperationOptions { signal: Some(signal.clone()) }),
        &turn,
    );
    signal.abort(Error::new("stop"));
    let fired = AbortSignal::new();
    fired.abort(Error::new("stop"));
    let fired_at =
        reacts_on(rig.i18n.set_locale("ar", OperationOptions { signal: Some(fired) }), &turn);
    rt::run_until_stalled();
    assert_eq!((ar_at.get(), fired_at.get()), ((1, true), (1, true)), "cancelled");

    let (rig, turn, _) = measure(vec![]);
    *rig.clock.borrow_mut() = Some(turn);
    rig.i18n
        .update_settings(&I18nConfig { locale: Some(Some("xx".into())), ..I18nConfig::default() });
    assert!(rig.warnings.borrow().is_empty());
    rt::run_until_stalled();
    assert_eq!(*rig.warned_on.borrow(), [1], "settings locale unknown");

    let (rig, _turn, heard) = measure(vec![("he", ready(he()))]);
    rig.i18n
        .update_settings(&I18nConfig { locale: Some(Some("he".into())), ..I18nConfig::default() });
    assert_eq!(rig.i18n.get_loading_locale().as_deref(), Some("he"));
    rt::run_until_stalled();
    assert_eq!(heard_list(&heard), [("changed he".to_string(), 1)], "settings locale lazy");
}

/// The memos compare inputs they hold, so a map freed between two reads
/// can't hand its address to the next one and pass for it.
#[test]
fn a_new_language_is_never_mistaken_for_a_freed_one() {
    let rig = Rig::with(JsRecord::new());
    for round in 0..200 {
        // Read, then let the answer go before the languages change.
        let before = rig.i18n.list_locales().len();
        let (a, b) = (format!("a{round}"), format!("b{round}"));
        rig.i18n.register_locale(pack(&a, &a, json!({})));
        rig.i18n.register_locale(pack(&b, &b, json!({})));
        let listed = rig.i18n.list_locales();
        assert_eq!(listed.len(), before + 2, "round {round}");
        let codes: Vec<String> =
            listed[listed.len() - 2..].iter().map(|l| l.code.clone()).collect();
        assert_eq!(codes, [a, b], "round {round}");
    }
    for round in 0..200 {
        // The same for the translator: new strings, a new translator. Its
        // text reads the live state either way; what a memoized component
        // sees is whether it is the same `Rc`.
        let previous = rig.i18n.get_translator();
        for half in ["x", "y"] {
            let text = format!("{half}{round}");
            rig.i18n
                .add_translations("en", serde_json::from_value(json!({ "only": text })).unwrap())
                .unwrap();
        }
        let now = rig.i18n.get_translator();
        assert!(!Rc::ptr_eq(&previous, &now), "round {round}");
        assert_eq!(now("only", None), format!("y{round}"));
    }
}

#[test]
fn a_remover_called_after_close_is_dropped_and_reported_once() {
    let rig = Rig::with(JsRecord::new());
    let remove = rig.i18n.register_locale(ar());
    rig.kernel.destroy();
    rt::run_until_stalled();
    drop(rig.i18n);
    remove();
    remove();
    let reports = rig.reports.borrow();
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert!(
        reports[0].starts_with("a state update after the instance closed (i18n#"),
        "{reports:?}"
    );
}

/// M37: the registration's loader table keeps only the loaders the
/// registered and current settings name, as the TypeScript store holds the
/// functions; a loader a change replaced is released, and the registered
/// ones stay for `resetSettings`.
#[test]
fn a_replaced_loader_is_released_from_the_loader_table() {
    let rig = Rig::with(loaders(vec![("he", ready(he()))]));
    assert_eq!(rig.i18n.retained_loaders(), 1);
    for _ in 0..50 {
        rig.i18n.update_settings(&I18nConfig {
            loaders: Some(loaders(vec![("fr", ready(pack("fr", "Français", json!({}))))])),
            ..I18nConfig::default()
        });
    }
    assert_eq!(rig.i18n.retained_loaders(), 2, "the registered loader and the current one");
    rig.i18n.reset_settings();
    rt::run_until_stalled();
    assert!(rig.i18n.get_settings().loaders.contains_key("he"), "the registered loader reads back");
}
