# epdf-plugin-i18n

`packages/plugin/i18n` in Rust, at the current contract. Every case replays identically
(`cargo test -p epdf-plugin-i18n`): 86 in `vectors/plugins/i18n/core.json` (translate,
interpolate, negotiateLocale, the seed and the named transitions), 532 in
`vectors/plugins/i18n/plurals.json`, and 24 scenarios in `vectors/plugins/i18n/plugin.json`. No
member-order allowance or outcome exception applies to them. `tests/behavior.rs` covers what no
vector prints, including the turns each switch settles on, measured on the TypeScript with `tsx`.

The plugin is workspace-scoped. Its capability works before `start`, while documents open and
close, and stops resolving after `destroy` (`alive-before-start-and-across-documents`).

The modules follow the TypeScript files: `contract.rs`, `model.rs` (pure transitions over an
`Rc<I18nState>`, returning the same `Rc` when nothing changes), `controller.rs`, `loaders.rs`
(`sync/loaders.ts`), `translate.rs`, `negotiate.rs` and `strings/` (EmbedPDF's own strings, each
language the TypeScript module printed as JSON). The settings live in the kernel's settings store:
`settings.rs` keeps each loader there as a handle, `{"$i18nLoader": n}`, into a table the
registration owns, so giving a code the loader it has is no change. `set_locale` rejects
`not-found`, `operation-failed` or `operation-cancelled` as the TypeScript's does. Superseding or
cancelling a switch rejects its waiter and never aborts the fetch; a pack that lands late registers
without switching. `get_translator` and `list_locales` return the same `Rc` until their inputs
change, as the TypeScript's `memo` does. `register_locale` returns a remover that unregisters the
code on every call, as the TypeScript's closure does, which is why it is an `Rc<dyn Fn()>` and not
a run-once `Unsubscribe`.

## What the TypeScript gets from its runtime

`translate.ts` calls `new Intl.PluralRules(locale).select(count)`. `src/plural.rs` reproduces it:
the tag is checked against ECMA-402's `IsStructurallyValidLanguageTag`, the canonical language is
matched against the languages V8 supports (an unsupported one resolves to `en-US`), and the
locale's CLDR rules are evaluated over ICU4C's operands. `src/plural_data.rs` is generated from Node
24.20.0 (ICU 78.3, CLDR 48.0), the environment `vectors/environment.json` records. We tried ICU4X's
`icu_plurals` 2.3.0 first; it failed 171 of the 528 cases the file had then, so it was dropped.

`negotiate.ts` reads `new Intl.Locale(code).maximize().script`. `src/script.rs` reproduces it over
`src/script_data.rs`, generated from the same Node: the script of every two- and three-letter
language subtag alone and with every two-letter and three-digit region, and where the unknown
script `Zzzz` breaks the simple rule. A cross-check of 40,000 random tags (scripts, regions,
variants, extensions, mixed case) against Node found no difference. The cost of both tables is that
they must be regenerated when Node's ICU moves; `vectors:check` will say so.

`src/js.rs` is the rest: `String(number)` for interpolated numbers, a record that enumerates
integer-like keys first, and the `Object.prototype` members a record lookup finds.
`setLocale('toString')` really does switch locale in the TypeScript, and `t('toString.name')`
really does return `"toString"`, so both are ported.

## What is not ported

Each of these is outside the TypeScript types or unreached by a vector, and is a Rust-only
divergence:

- A settings change compares `locales` by value. `followSettings` registers a language that was
  not in the previous list by identity (`previous.locales.includes(locale)`); the store holds JSON,
  so Rust treats an equal language as the same one and does not re-register it.
- `script_of` does not apply the legacy aliases keyed by a language and a variant together. Only
  `art-lojban` changes the answer: ICU says `Latn` (as `jbo`), Rust says no script, which
  negotiation reads as "can't tell".
- `addTranslations` for a code naming an `Object.prototype` member passes the check in both, and the
  TypeScript merges into a pack made from that member; the Rust transition leaves the state as it
  is.
- A `__proto__` locale code sets the record's prototype in the TypeScript. A dotted key that walks
  past a function member further than `.name` reads as absent here. `params` values other than
  strings and numbers aren't accepted. A loader that resolves to something that isn't a locale
  fails the load in Rust, where the TypeScript throws inside its `then`.
- The function `get_translator` returns holds the plugin weakly and answers the key once the plugin
  is gone; the TypeScript's closure keeps the controller alive.

The harness replays every member a vector calls. `getSettings`, `onSettingsChanged` and
`getTranslator` answer with functions the replay does not print, so the harness refuses them by
name; `tests/behavior.rs` covers them through the typed API.

`src/ffi.rs` and `tests/ffi.rs` are migrated by hand and not compiled until Task 13.33 rebuilds
`epdf-ffi`.
