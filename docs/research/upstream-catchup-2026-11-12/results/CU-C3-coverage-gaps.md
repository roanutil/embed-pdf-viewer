# CU-C3 coverage gaps in upstream's new code

Lines that upstream added in `074a190c9..ed064e0c8` and that no plugin vector reaches, from
`vectors/coverage.json` at `3b8cf99eb` (`pnpm run coverage`). Each line range is followed by its count
and the upstream commits `git blame` at `ed064e0c8` gives for those lines. Only ported packages are
measured (`packages/core/{main,geometry,stage,ui,annotation,acrojs,signature}` and `packages/plugin/*`);
engine code is covered by the engine vectors, listed in the report. Produced by
`scripts/cu-c3/gaps.py`.

## `core/annotation` (CU-C5)

- `packages/core/annotation/src/update/index.ts`: lines 237 (1; f0bb7b047)

## `core/main` (CU-C6)

- `packages/core/main/src/changes.ts`: lines 483-487,490-491 (7; b3f1c447e)
- `packages/core/main/src/kernel.ts`: lines 1386-1387 (2; a4805a492)
- `packages/core/main/src/page-mirror.ts`: lines 243 (1; 9b5125408)
- `packages/core/main/src/testing.ts`: lines 168,320,330,338-339 (5; 8bc6546e5)

## `core/ui` (CU-C6)

- `packages/core/ui/src/layout-compile.ts`: lines 407,422 (2; 7af98f4df)

## `plugin/annotation` (CU-C5)

- `packages/plugin/annotation/src/read/annotations.ts`: lines 146-148 (3; 1481e4556)
- `packages/plugin/annotation/src/read/render.ts`: lines 290,350-362,373-377,387-399 (32; 1481e4556 9eead5531 db8b5ffb1)
- `packages/plugin/annotation/src/services/authority.ts`: lines 84-93 (10; 1481e4556)
- `packages/plugin/annotation/src/services/copy-sources.ts`: lines 36-37 (2; db8b5ffb1)
- `packages/plugin/annotation/src/services/staging.ts`: lines 42,73-79,90,226-257 (41; 1481e4556 9eead5531 db8b5ffb1)
- `packages/plugin/annotation/src/services/store.ts`: lines 286-294,462-472,491-493,531-536 (29; 1481e4556 9eead5531 a390171cc db8b5ffb1)
- `packages/plugin/annotation/src/sync/records.ts`: lines 272-305,392-407,464-465 (52; 1481e4556 9eead5531)
- `packages/plugin/annotation/src/tools/definitions.ts`: lines 713 (1; f0bb7b047)
- `packages/plugin/annotation/src/tools/registry.ts`: lines 175-179 (5; f0bb7b047)
- `packages/plugin/annotation/src/write/crud.ts`: lines 189-258,283,286-302,306-310 (93; 1481e4556 a390171cc b3f1c447e f8ac5a351)
- `packages/plugin/annotation/src/write/selection.ts`: lines 69-74,81-92,297-309,311-325 (46; 1481e4556 a390171cc)

## `plugin/commands` (CU-C6)

- `packages/plugin/commands/src/controller.ts`: lines 352 (1; 7af98f4df)
- `packages/plugin/commands/src/services/availability.ts`: lines 96,105,107,113,148,152,194 (7; 7af98f4df)

## `plugin/form` (CU-C5)

- `packages/plugin/form/src/sync/predict.ts`: lines 139,252-285,291-314,332,419-427,430 (70; a390171cc)
- `packages/plugin/form/src/write/duplicate.ts`: lines 63-69,72,82-218 (145; a390171cc)
- `packages/plugin/form/src/write/fields.ts`: lines 109-113,206-208,216-233,236-262,265-276,336-366 (96; a390171cc)
- `packages/plugin/form/src/write/new-widget.ts`: lines 13-17 (5; a390171cc)

## `plugin/measurement` (CU-C5)

- `packages/plugin/measurement/src/sync/viewports.ts`: lines 45,93 (2; 8bc6546e5)
- `packages/plugin/measurement/src/write/scale.ts`: lines 62-68,110,120,144 (10; 8bc6546e5)

## `plugin/stage` (CU-C6)

- `packages/plugin/stage/src/camera/coasting.ts`: lines 55-72,75-79 (23; 138c54fe7)
- `packages/plugin/stage/src/camera/wheel.ts`: lines 46-53,58-79 (30; 13305a14b 138c54fe7)
- `packages/plugin/stage/src/navigation/turn.ts`: lines 144-145,180-186 (9; 13305a14b 138c54fe7)

## Files upstream touched that no vector loads

- `packages/core/annotation/src/types.ts` (type-only module, CU-C5)

The largest gaps are the two copy features, annotation duplicate (`1481e4556`, follow-ups
`9eead5531`, `db8b5ffb1`, `f8ac5a351`) and form field duplicate (`a390171cc`): no generator case
copies anything yet.
