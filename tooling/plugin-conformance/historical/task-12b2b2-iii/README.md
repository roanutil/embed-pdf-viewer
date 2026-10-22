# Form scripting and actions oracle history

The exact definitions at `c85d40a5a15d75c87f9be8ee0e75bf9038fec812` are
`form-scripting.ts.txt` and `form-actions.ts.txt`. `original-inputs.json`
preserves all 46 + 13 complete canonical inputs; `baseline.json` pins the
source bytes. These inputs equal their rows in the immutable original220
archive in `../task-12b2b2-i/`. `accounting.json` records each before/current
name, input digest, intent and shipping source evidence. `family.json`
combines the approved i101 + ii60 + these59 mappings: original220 → current221
(core99 pure + lifecycle3 + fill31 + design29 + scripting46 + actions13).
All original names survive, there are zero retired/renamed cases, and the sole
added lifecycle case belongs to i. Task11 records inputDiff0 in every form
family; none of these migrations reclassifies that drift.

## Current ownership and deliberately replaced distinctions

`98e5c270c` removes `forms.applyEffects`. `5da4ca52a` gives every fill one held
change and makes the script controller consume an explicit current snapshot
and return effects. `1233d1349` moves widget reads and action trees into the
form snapshot; `cdf4586c7` supplies current refs and public names. The active
cases invoke these actual current owners. They do not invoke the archived
batch statuses, old field types, aliases, frozen reducers or missing methods.

The 35 standalone cases return accepted/rejected/failed script results;
accepted with no effects is the controller's no-op. Native applied/unchanged
and atomic refusal belong to the other 24 integration cases. The standalone
driver creates the current controller over an actual detached actions realm;
its serialized arguments already contain the complete current snapshot,
including widget rows, field relationships and calculation order. At case
construction `controllerInputs` validates and binds that snapshot from the
authored `forms.list` fixture into each call's args. It performs no runtime
conversion and the driver neither reads nor writes the form. The authored
snapshot table is retained as construction data. Native cases use the
reviewed form-current factory and current field/widget constructors.

The three realms in `rejection-keeps-boot-effects` each preserve boot effects
on refusal. Their removed per-effect native batch distinctions are archived;
actual whole-change success/unchanged/engine refusal are checked in
`native-effect-statuses`, the reset executor and the commit-sink cases.
Current setValues/importValues validate each native/plain value and run one
recalculation, rather than each field's validator. A direct single-field
validation refusal plus partial batch failures cover the surviving channels.
Submit and reset read the mirrored current form without forced rereads.
The script sink acknowledges staging immediately; an engine refusal later
rolls back the mirror, while denied fill authority refuses before staging.

Current baseIndex and objectNumber addresses replace removed nm/index/
annotObjectNumber addresses. `a-kindless-address-matches-no-candidate` uses a
valid baseIndex to preserve the no-candidate refusal; its malformed historical
address is preserved exactly in the archive. Action trees and hover flags
come from form-owned rows even with annotation lending. They are no longer
fetched from a folded annotation copy. Object-number lookup currently allows
another page's number through the form ownership join; the dispatcher sees
no row on that page and returns to the form's unchanged path. This observed
behavior is explicitly asserted rather than silently tightened.

## Host inputs and cleanup

`form-script-fixtures.ts` owns an isolated current actions registration and a
small typed scripted sandbox. It logs factory/boot/run/dispose inputs and
consumes authored answers in order, repeating the last answer. It implements
no field, script, action or engine policy. Current shipping actions services
own identity, clock, seed, transactions, budgets and UI decisions. Typed
native values and complete static answers are prepared before execution;
no answer examines a live request. The fixture's only config assertion is
the existing scenario registry's unknown → declared fixture config boundary.
Form and action methods/argument tuples are checked against current host
capabilities; callback literals describe the runner's callable host seam.

The diagnostic factories install in this order: scripting console host, form
console host, then annotation console host. Their case-end hooks restore in
the opposite order. The scripting host records only `[form]` warnings and
`[actions]` errors; the form host records `[form]` errors. Ordinary runs,
refusals, construction failures after every wrapper order, early getter
failure and repeated full traces verify exact console descriptors/identity,
crypto descriptor/prototype and shared entropy restoration. Neither deferred
legacy fixture is changed.

## Task13 reconstruction and explicit carries

Keep schema2 and all221 form cases, including the lifecycle suite. Replay
complete current snapshots, scalar/array/none values, current field and widget
refs, ordered effect lists, actual ChangeOps/ChangeResult items and own-field
facts with exact transaction indexes/counts. Standalone results carry boot
and user UI phases, exact diagnostics and execution errors; failed budgets
retain already observed UI requests, but discard native effects. Preserve
UTF-16 splicing, UTF-8 output-byte accounting, 50→30ms remaining budgets,
realm rebuild/boot behavior and the current default/overridden identity
fields. The generator pins TZ=UTC; host-zone offsets in direct tests remain
explicit host inputs. Native script writes use the same shared UUID sequence
as annotation and the form queue, never a new per-factory sequence.

The current form-pipeline lifecycle print reaches the adapter despite the
print lifecycle policy row being `block`, when no kernel printer exists.
An action-executor lifecycle print consults that row, emits the public blocked
diagnostic and never calls the adapter. Preserve both observed paths under
the existing M14/form-print rulings; there is no production convergence here.
The previously verified no-annotation placement refusal/unhandled edge and
paged empty-stage limitation remain separate carries. No source package,
Rust or vector is changed by this migration.

The legacy form helper remains byte-identical for stamp-placement, stamp-api
and signature-plugin. The annotation bridge remains for that helper,
measurement, redaction, stamp-placement, stamp-api, stamp-fixtures,
signature-plugin and link. Deferred package compiler errors remain68:
annotation bridge3 + form bridge12 + B3 21 + C32. D owns final bridge removal
or repair, package compiler0 and fresh global runtime closure. Parent forms
closure requires independent review; these artifacts do not claim B3/C/D or
whole-package closure.
