# epdf-plugin-i18n

`packages/plugin/i18n` in Rust, at the v3 contract. Every case replays identically
(`cargo test -p epdf-plugin-i18n`): 79 in `vectors/plugins/i18n/core.json` (translate,
interpolate, negotiateLocale, the reducer and initial state), 532 in
`vectors/plugins/i18n/plurals.json`, and 24 scenarios in `vectors/plugins/i18n/plugin.json`.

The plugin is workspace-scoped, and its capability works before `start`, while documents open and
close, and stops resolving after `destroy` (`alive-before-start-and-across-documents`).

The modules follow the TypeScript files: `contract.rs`, `model.rs`, `controller.rs` and
`loaders.rs` (`sync/loaders.ts`). `create_i18n_controller` returns the api and a `connect` that
starts the loader watch and the `onLocaleChanged` announcer. `set_locale` returns a `Promise<()>`
that rejects `not-found`, `operation-failed` or `operation-cancelled` as the TypeScript's does, and
`register_locale` returns a remover that dispatches `UNREGISTER_LOCALE` on every call, as the
TypeScript's closure does. `listLocales` rebuilds its list on every call; the TypeScript caches it
per `locales` object, which only reference identity could observe. A failing event listener goes to
stderr, where the TypeScript writes to `console.error`; neither reaches a trace.

## What the TypeScript gets from its runtime

`translate.ts` calls `new Intl.PluralRules(locale).select(count)`, and that is most of this crate.
`src/plural.rs` reproduces it in three steps. The tag is checked against ECMA-402's
`IsStructurallyValidLanguageTag`, and an invalid one takes the TypeScript's `count === 1` catch
arm. The canonical language is matched against the languages V8 supports, and an unsupported one
resolves to the default locale, `en-US`. Then the number is formatted the way `select` formats it
(shortest digits, at most three fraction digits, half away from zero) and the locale's CLDR rules
are evaluated over ICU4C's operands, including its truncation of the integer operand to the lowest
18 digits, which is why `1e21` is `one` in French.

`src/plural_data.rs` is generated, not written: the alias and support tables come from enumerating
every two- and three-letter language subtag under Node 24.20.0 (ICU 78.3, CLDR 48.0), and the rules
are CLDR 48.0.0's `supplemental/plurals.json` from `cldr-core`. `plurals.json` pins all 424 language
codes V8 accepts plus scripts, regions, extensions, unsupported tags and 36 invalid ones, across 50
counts each. We tried ICU4X's `icu_plurals` 2.3.0 first. It failed 171 of the 528 cases the file
had then, mostly languages its compiled data leaves out (it ships the modern CLDR locales only) and
the rest the `1e21` operand, so it was dropped. The cost of our own table is that it has to be
regenerated when Node's ICU moves, and `vectors:check` is what will say so.

`src/js.rs` is the rest: `String(number)` for interpolated numbers, a record that enumerates
integer-like keys first (`locales()` order), and the `Object.prototype` members a record lookup
finds. `setLocale('toString')` really does switch locale in the TypeScript, and
`t('toString.name')` really does return `"toString"`, so both are ported and pinned.

## Config

A vector's config is JSON, so `loaders` are `$fn` specs and the warn-once signal, which the
TypeScript sends to `console.warn`, goes to `I18nConfig::warn`; the harness logs it as the callback
`console.warn`, where the generator records the TypeScript's. The Rust runner reads objects in key
order, so the generator refuses a vector whose `loaders` aren't written in key order.

## Coverage and what is not ported

The i18n suites reach 618 of the package's 625 statements and 153 of its 154 branches. What they
don't reach:

- `controller.ts:168-174`, `createI18nCapability`, the helper the TypeScript's unit tests build a
  capability with. No plugin calls it. It is ported as `create_i18n_capability` and not replayed.
- `sync/loaders.ts:25`, the `if (!loader) return` guard: `loading` only ever names a code with a
  loader.

Some inputs are outside the port, each outside the TypeScript types. A `__proto__` locale code in
`config.locales` sets the record's prototype in the TypeScript. A dotted key that walks past a
function member further than `.name` (`toString.call.name`, `constructor.keys.name`) reads as
absent here. `params` values other than strings and numbers aren't accepted. A loader that resolves
to something that isn't a locale rejects in Rust and is dispatched as `LOAD_FAILED`, where the
TypeScript throws inside its `then`. `addTranslations('toString', ...)` passes the TypeScript's
registered check and merges into a pack made from a function; the Rust reducer ignores it.

Mutation numbers are pre-v3, rerun pending. `cargo mutants -p epdf-plugin-i18n --test-package epdf-plugin-i18n` caught 280 of 284 viable
mutants (277 caught, 3 timeouts, which are loops the mutant makes infinite). The four survivors
are equivalent. Three are in `number_to_string` (`js.rs:186`, `188`, `201`): `>` or `<` becoming
`>=` or `<=` on a value that can't be zero there (the infinity arm, the sign after the zero
return, and an exponent that is at least 21 or at most -7). The fourth replaces the record
deserializer's `expecting`, which only words a parse error.
