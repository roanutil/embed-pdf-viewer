# Annotation pure/repository migration history

BASE `0d4199872`. The adjacent `.ts.txt` files preserve all 850 original
function definitions and their input builders. They are historical text,
not generator modules. Their shared legacy fixture definitions remain in
`src/suites/annotation-fixtures.ts` at BASE. The new pure suites use the
separate, typed `annotation-current-fixtures.ts`; live scenarios still use
the old helper until Task12b2b1b.

`accounting.json` maps every original name to its current name or an evidenced
retirement, records its original function and argument digest, identifies
all 573 repository / 23 pure Task11 input drifts, and lists the one added
current case. Argument digests are SHA256 of the captured canonical argument
array serialized with Python `json.dumps(sort_keys=True, separators=(',', ':'),
ensure_ascii=False)`. Full before/after arguments and outcomes are retained
in the ignored task reports; the BASE commit reproduces historical inputs.

The 842 mapped active definitions plus one added case give 843 current
cases. Eight definitions retire:

| Definitions                                                          | Upstream evidence and surviving coverage                                                                                                                                                                                               |
| -------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `fromDTO/vector-source`, `fromDTO/default-source`                    | d29344e88 `core/annotation/src/record/index.ts:98-116` already takes one DTO and always reads baked. `fromDTO/square` covers the record; live text fixtures explicitly choose current `source:'vector'` before recording.              |
| `toCreateDraft/unsupported`                                          | b979f2aef removes the old writer; current `plugin/annotation/src/write/crud.ts:72-79` refuses unsupported creates. Current unsupported record/shape/field-diff cases remain.                                                           |
| `insetPdfRectByRD/inset`, `/none`                                    | 2bdc74fbf removes `unrotatedRect`, `rectDifferences` and viewer `/RD` math. Current `record/shape.ts` reads the engine's `box`. Upright/rotated/distinct-box callout and free-text cases remain.                                       |
| `annotationReducer/set-hydration`, `/unknown-action`, `/stamp-armed` | 4fc1a32af removes reducer/dispatch, hydration state and `stampArmEpoch`. Current named transitions and settings cover state changes and no-op identities. Hydration and stamp resources require live scenario coverage in Task12b2b1b. |

Every other removed helper has a shipping replacement: full patches use
`annotationPatchBetween` on actual before/after DTOs; geometry/property
writes use `withShape` / `withValues`; drawing creates use `draftOf`;
`shapeOf`, `styleOf`, `widgetAppearanceOf`, `colorOf`, `rgbOf` and
`pagePointsBounds` replace the old projections. Actual current arguments
are constructed before `FunctionCase.args` is recorded. No removal-error
function or copied projection implementation remains active.

Pure replacements use named annotation model transitions, shared
`mergeSettings`, and the current engine `AnnotationDraftSchema.parse`.
The schema-only target intentionally reports its shipping behavior; it
does not recreate removed convenience geometry validation. Font lookup and
byte-buffer wrappers retain only their existing serializable host ports.

Schema-invalid imported-read edges remain explicit: missing/null refs,
one-point callout, NaN rotation, empty color, absent rich text, and empty
review-state strings. Other deliberately adversarial inputs include
nonfinite geometry and non-hex colors. Focused tests allow only named
refusals and require every finite-input result to be finite.
