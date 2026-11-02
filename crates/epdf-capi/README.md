# Native C ABI lifecycle

This crate provides owned engine, job and result handles over an owner-thread
runtime. The public header is [`include/epdf.h`](include/epdf.h). It is an
unreleased, lifecycle-only binding: document operations and pure helper opcodes
are reserved and return an explicit `NotImplemented` engine error. It does not
yet provide a usable PDF reading/rendering API or a managed package.

## Ownership and control

- Create an engine, submit bounded work, collect a job once, and release the
  job. Release every nonzero result/error handle, including failure results.
- Close seals admission and drains accepted work. A timeout leaves it closing;
  repeat close to await disposal. Release invalidates the engine handle and
  requests the drain without joining, for safe finalizer use.
- Cancellation signals the job directly. It is cooperative and does not
  interrupt arbitrary native calls. Releasing a job abandons its result and
  signals cancellation. Process faults are not isolated.
- Results outlive their engine until released. A view is borrowed, immutable
  memory; do not race view access with release or free it with another allocator.
- Opaque tagged generation handles reject stale/wrong-kind/wrong-parent IDs.
  Invalid memory addresses are still a C caller contract violation. Input spans
  and aligned output storage must be valid and must not overlap.

Every C export contains unwinding, and each owner job catches failure through
result framing/completion. A panic closes/poisons the engine and completes
pending work. Its status remains `Panicked` even if no error buffer can fit.

## Creation options

Options are UTF-8 JSON, or an empty span for defaults. Unknown fields are
rejected. `limits` accepts the following camelCase settings:

| Setting                | Default    |
| ---------------------- | ---------- |
| maxPendingJobs         | 16         |
| maxJsonBytes           | 8 MiB      |
| maxPayloadBytes        | 256 MiB    |
| maxQueuedInputBytes    | 256 MiB    |
| maxRetainedInputBytes  | 1 GiB      |
| maxResultBytes         | 256 MiB    |
| maxRetainedResultBytes | 512 MiB    |
| maxOutputPixels        | 16,777,216 |

Settings must be positive and fit the checked ABI/framing ceilings. Creation
options have an 8 MiB bootstrap JSON limit. Result frames across all engines
and helpers also share a 1 GiB ceiling, so one engine's retained results can
exhaust another's capacity. Dispose results promptly. These budgets do not
bound allocator overhead or PDFium's own internal allocations.

`startupFonts` is an ordered array of `{key, payloadIndex, familyName?, weight?,
italic?, fallback?}`. Use unique keys and exactly one distinct payload per font.
The binary payload frame contains their bytes. Omitted family/weight/italic
infer font metadata; `italic` is boolean or null. All fonts and fallbacks are
registered on the owner thread before creation returns. A failed registration
tears down that state on the same thread. Font memory stays charged until
owner-thread disposal.

## Build and manual validation

Use the rebuilt project dev container and pinned runtime artifacts:

```sh
cargo test -p epdf-capi
cargo test -p epdf-engine --lib dispatch::tests
cargo build -p epdf-capi
mkdir -p target/manual
clang -std=c11 -Wall -Wextra -Werror -Icrates/epdf-capi/include \
  crates/epdf-capi/tests/c_smoke.c -Ltarget/debug -lepdf_capi \
  -Wl,-rpath,"$PWD/target/debug" -o target/manual/c-smoke
target/manual/c-smoke
```

Record source/runtime/font pins, image or host environment, commands, artifact
SHA256s and outcomes using [`platforms/dotnet/validation`](../../platforms/dotnet/validation/README.md).
No `.github/**` changes, hosted checks, package publication or release action
is part of this slice. Only environments with recorded evidence are validated.
