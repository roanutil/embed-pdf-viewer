//! G4's font gate: registration, fallback order, clear and re-register.
//!
//! The port order asks for a "font-before-first-job test", "clear/re-register
//! and fallback-order tests across both paths", and a coexistence test that
//! registers once and renders through both backends. The last is web work and
//! is not here; the rest are.
//!
//! Every test runs on its own thread. PDFium's font registry is thread-global
//! and so is the one-registry rule, so two tests sharing a thread would share
//! a registry and see each other's fonts. Cargo runs tests on separate
//! threads, but that is a scheduling detail rather than a promise, so each
//! test spawns its own and joins it.

use epdf_engine::features::fonts::{FontRegistry, StartupFont};
use pdfium::FontDescription;

/// Run `body` on a thread of its own, so its registry is the only one there.
fn on_a_fresh_thread<T: Send + 'static>(body: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::spawn(body).join().expect("the test thread panicked")
}

fn roboto_path() -> std::path::PathBuf {
    test_support::workspace_root().join("packages/engine/main/test/fixtures/Roboto-Regular.ttf")
}

fn roboto_bytes() -> Vec<u8> {
    std::fs::read(roboto_path()).expect("read Roboto-Regular.ttf")
}

#[test]
fn a_font_registered_from_bytes_resolves_by_key() {
    on_a_fresh_thread(|| {
        let mut registry = FontRegistry::new().expect("registry");
        let id = registry
            .register("roboto", &FontDescription::inferred(), &roboto_bytes())
            .expect("register");

        assert!(registry.contains("roboto"));
        assert_eq!(registry.id_for("roboto").expect("resolve"), id);
        assert_eq!(registry.len(), 1);
    });
}

#[test]
fn a_font_registered_from_a_file_resolves_by_key() {
    on_a_fresh_thread(|| {
        let mut registry = FontRegistry::new().expect("registry");
        let id = registry
            .register_file("roboto", &FontDescription::inferred(), &roboto_path())
            .expect("register");
        assert_eq!(registry.id_for("roboto").expect("resolve"), id);
    });
}

#[test]
fn an_unregistered_key_is_refused_rather_than_guessed() {
    on_a_fresh_thread(|| {
        let registry = FontRegistry::new().expect("registry");
        let error = registry.id_for("nobody").expect_err("not registered");
        assert!(error.message.contains("nobody"), "{}", error.message);
    });
}

#[test]
fn empty_and_corrupt_font_data_are_refused() {
    on_a_fresh_thread(|| {
        let mut registry = FontRegistry::new().expect("registry");
        assert!(registry.register("empty", &FontDescription::inferred(), &[]).is_err());
        assert!(
            registry
                .register("garbage", &FontDescription::inferred(), b"this is not a font")
                .is_err()
        );
        assert!(registry.is_empty(), "a failed registration left a key behind");
    });
}

#[test]
fn a_missing_font_file_is_refused_and_names_the_path() {
    on_a_fresh_thread(|| {
        let mut registry = FontRegistry::new().expect("registry");
        let missing = test_support::workspace_root().join("no-such-font.ttf");
        let error = registry
            .register_file("missing", &FontDescription::inferred(), &missing)
            .expect_err("no file");
        assert!(error.message.contains("no-such-font.ttf"), "{}", error.message);
        assert!(registry.is_empty());
    });
}

#[test]
fn the_fallback_chain_keeps_the_order_it_was_given() {
    // Order fixes precedence, so two workers running the same list have to
    // resolve a missing glyph the same way.
    on_a_fresh_thread(|| {
        let mut registry = FontRegistry::new().expect("registry");
        for key in ["first", "second", "third"] {
            registry
                .register(key, &FontDescription::inferred(), &roboto_bytes())
                .expect("register");
            registry.add_fallback(key).expect("fallback");
        }
        assert_eq!(registry.fallback_order(), ["first", "second", "third"]);
    });
}

#[test]
fn a_fallback_for_an_unregistered_key_is_refused() {
    on_a_fresh_thread(|| {
        let mut registry = FontRegistry::new().expect("registry");
        assert!(registry.add_fallback("nobody").is_err());
        assert!(registry.fallback_order().is_empty());
    });
}

#[test]
fn clearing_fallbacks_leaves_the_fonts_registered() {
    // The two are separate operations on purpose: a caller reordering its
    // fallback chain should not have to re-register every font to do it.
    on_a_fresh_thread(|| {
        let mut registry = FontRegistry::new().expect("registry");
        registry
            .register("roboto", &FontDescription::inferred(), &roboto_bytes())
            .expect("register");
        registry.add_fallback("roboto").expect("fallback");

        registry.clear_fallbacks();
        assert!(registry.fallback_order().is_empty());
        assert!(registry.contains("roboto"), "clearing fallbacks unregistered a font");
        registry.add_fallback("roboto").expect("re-add");
    });
}

#[test]
fn clear_removes_every_font_and_re_registration_works_afterwards() {
    on_a_fresh_thread(|| {
        let mut registry = FontRegistry::new().expect("registry");
        registry
            .register("roboto", &FontDescription::inferred(), &roboto_bytes())
            .expect("register");
        registry
            .register_file("roboto-file", &FontDescription::inferred(), &roboto_path())
            .expect("register");
        registry.add_fallback("roboto").expect("fallback");

        registry.clear();
        assert!(registry.is_empty());
        assert!(registry.fallback_order().is_empty());
        assert!(registry.id_for("roboto").is_err());

        // And the registry is usable again, through both paths.
        registry
            .register("roboto", &FontDescription::inferred(), &roboto_bytes())
            .expect("re-register from bytes");
        registry
            .register_file("roboto-file", &FontDescription::inferred(), &roboto_path())
            .expect("re-register from a file");
        assert_eq!(registry.len(), 2);
    });
}

#[test]
fn startup_fonts_register_in_order_and_join_the_chain() {
    on_a_fresh_thread(|| {
        let mut registry = FontRegistry::new().expect("registry");
        let fonts = vec![
            StartupFont {
                key: "body".to_string(),
                path: roboto_path(),
                description: FontDescription::inferred(),
                fallback: true,
            },
            StartupFont {
                key: "display".to_string(),
                path: roboto_path(),
                description: FontDescription::inferred(),
                // Registered but not a fallback, which is the distinction the
                // flag exists for.
                fallback: false,
            },
        ];
        registry.register_startup(&fonts).expect("startup");

        assert_eq!(registry.keys().collect::<Vec<_>>(), ["body", "display"]);
        assert_eq!(registry.fallback_order(), ["body"]);
    });
}

#[test]
fn a_second_registry_on_one_thread_is_refused() {
    // PDFium's registry is thread-global, so two owners would clear each
    // other's fonts and release each other's file handles. The rule is in the
    // port order; this makes breaking it fail at the second construction
    // rather than silently at the first clear.
    on_a_fresh_thread(|| {
        let first = FontRegistry::new().expect("the first registry");
        let refused = FontRegistry::new().expect_err("a second on the same thread");
        assert!(refused.message.contains("thread-global"), "{}", refused.message);

        // And dropping the first releases the claim.
        drop(first);
        FontRegistry::new().expect("a registry after the first one went");
    });
}

#[test]
fn each_thread_gets_its_own_registry() {
    // The other half of thread-locality: a font registered on one thread is
    // invisible on another, which is exactly why ids never cross.
    on_a_fresh_thread(|| {
        let mut registry = FontRegistry::new().expect("registry");
        registry
            .register("roboto", &FontDescription::inferred(), &roboto_bytes())
            .expect("register");
        assert!(registry.contains("roboto"));

        let elsewhere = on_a_fresh_thread(|| {
            let other = FontRegistry::new().expect("registry");
            other.contains("roboto")
        });
        assert!(!elsewhere, "a font leaked between threads");
    });
}

#[test]
fn a_registry_can_be_filled_before_any_document_is_open() {
    // The font-before-first-job test. Fonts are engine-level, so registering
    // has to work with nothing open.
    on_a_fresh_thread(|| {
        let mut registry = FontRegistry::new().expect("registry");
        registry
            .register("roboto", &FontDescription::inferred(), &roboto_bytes())
            .expect("register before anything is open");
        registry.add_fallback("roboto").expect("fallback before anything is open");

        // And a document opened afterwards works normally.
        let document = pdfium::Document::from_bytes(
            pdfium::Library::acquire(),
            &test_support::fixture(test_support::REPORT),
            None,
        )
        .expect("open");
        assert_eq!(document.page_count(), 14);
        assert!(registry.contains("roboto"), "opening a document disturbed the registry");
    });
}
