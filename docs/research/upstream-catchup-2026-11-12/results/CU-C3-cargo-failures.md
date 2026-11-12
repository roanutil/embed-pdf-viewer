# CU-C3 cargo failures, grouped by port job

`cargo test --workspace --no-fail-fast` on `research/catchup-2026-11-12-plugins` at `b8ade7fe8`: 3,269
passed, 23 failed, 1 ignored (`results/CU-C3-after/cargo.log`). At the start, `7990560d6`, it was 3,292
passed and 0 failed. The workspace builds with no change to Rust: every failure is a plugin conformance
replay of a vector regenerated in `3b8cf99eb`, and each cause below names the upstream commit its diff
traces to. `epdf-engine` has no failures, since the engine vectors changed only in `wire/requests.json`.

## CU-C4 (engine): none

No engine-level crate fails. CU-C4's work is the new engine behaviour that has no engine vector case
yet; see the report's coverage section.

## CU-C5 (annotation and form family): 10

| Crate | Test | Cause |
|---|---|---|
| `epdf-annotation-core` | `update_matches_the_typescript` | drafts carry `drawing`; an ink stroke under the pen is its points (`59864ae01`) |
| `epdf-annotation-core` | `view_matches_the_typescript` | same, and the drawn stroke's box is no longer inflated by the curve (`59864ae01`) |
| `epdf-plugin-annotation` | `api_matches_the_typescript` | `remeasurePage` became `whenRemeasurable` and synchronous `stageRemeasure` (`8bc6546e5`) |
| `epdf-plugin-annotation` | `hydration_matches_the_typescript` | `stream.desynced` reads `pages.list` again (`a4805a492`) |
| `epdf-plugin-annotation` | `pure_functions_match_the_typescript` | tool entries apply in order; `extends` names an earlier tool (`f0bb7b047`) |
| `epdf-plugin-annotation` | `tools_match_the_typescript` | `drawing` on drafts (`59864ae01`); a registration's remover puts back the tool it replaced (`f0bb7b047`) |
| `epdf-plugin-form` | `fill_matches_the_typescript` | `pages.list` on `stream.desynced` (`a4805a492`); `forms.import` carries its `opId` (`b3f1c447e`) |
| `epdf-plugin-measurement` | `scenarios_match_the_typescript` | scales ride on the page layout; a scale change is one `doc.apply` change with its re-measure; the viewport mirror, its load errors and the per-page queue are gone (`c7f8d75ff`, `8bc6546e5`) |
| `epdf-plugin-measurement` | `functions_match_the_typescript` | `loadErrors`, `clearLoadError` and `pageScaleOf` removed (`8bc6546e5`) |
| `epdf-plugin-signature` | `the_plugin_matches_the_typescript` | `pages.list` on `stream.desynced` (`a4805a492`) |

## CU-C6 (kernel, stage and the rest): 13

| Crate | Test | Cause |
|---|---|---|
| `epdf-plugin-actions` | `triggers_match_the_typescript` | `pages.list` on `stream.desynced` (`a4805a492`) |
| `epdf-plugin-attachment` | `attachment_matches_the_typescript` | stage setting `scrollChaining` in the stage state it snapshots (`13305a14b`) |
| `epdf-plugin-commands` | `the_plugin_matches_the_typescript` | derivations asked at most once per store change, in the order enabled, visible, iconAccent, active (`7af98f4df`) |
| `epdf-plugin-link` | `scenarios_match_the_typescript` | `scrollChaining` (`13305a14b`) |
| `epdf-plugin-outline` | `outline_matches_the_typescript` | `scrollChaining` (`13305a14b`) |
| `epdf-plugin-search` | `stage_integration_matches_the_typescript` | `scrollChaining` (`13305a14b`) |
| `epdf-plugin-selection` | `selection_matches_the_typescript` | the page mirror's store cell holds a version number, not the entries (`9b5125408`) |
| `epdf-plugin-stage` | `capability_matches_the_typescript` | `scrollChaining` (`13305a14b`) |
| `epdf-plugin-stage` | `change_hooks_match_the_typescript` | `scrollChaining` (`13305a14b`) |
| `epdf-plugin-stage` | `feeds_match_the_typescript` | `scrollChaining` (`13305a14b`) |
| `epdf-plugin-stage` | `motion_matches_the_typescript` | `scrollChaining` (`13305a14b`) |
| `epdf-plugin-stage` | `page_turns_match_the_typescript` | `scrollChaining` (`13305a14b`) |
| `epdf-plugin-stage` | `pure_modules_match_the_typescript` | `scrollChaining` in the settings, defaults and `SETTING_KEYS` (`13305a14b`) |

Every crate named here is on the brief's lists, so no crate had to be assigned.
