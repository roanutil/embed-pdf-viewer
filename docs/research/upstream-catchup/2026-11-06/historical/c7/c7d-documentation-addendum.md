# C7d documentation-only change record

Updated `scope-and-plan.md` near its top with five reader-facing viewer change groups and a separate runtime A/B explanation. The actual completed upstream baseline is **a40d63a199f2d483fd27c9ad439f0d5efbc195aa**, not private tip f28ed. The earlier measured plan target was **74afd55753ee7e6b34c57e3dc2e1a683b4c5a23b**; retained C5g source/target records identify the actual merge50786828f, and C7 merge-base/history agrees. The fixed new viewer target is **4ad8986099314b9587df423c802db103a21d665d**.

| Version / evidence | Path | SHA256 |
|---|---|---|
| Historical exact original, preserved before edit | `/private/tmp/kkt-peach-lemur/c7/scope-and-plan.original-bcf94c0cee6b9f98.md` | `bcf94c0cee6b9f98d90e5375bb5c32d866b9cec3ca398fb0fca7e0a98da18fb0` |
| Current live plan | `/private/tmp/kkt-peach-lemur/c7/scope-and-plan.md` | `02a116ee49861681c40e75fd1fc6bb610085af1ac1bde5a9e53176e67e663599` |
| Current immutable version | `/private/tmp/kkt-peach-lemur/c7/scope-and-plan.c7d-02a116ee49861681.md` | `02a116ee49861681c40e75fd1fc6bb610085af1ac1bde5a9e53176e67e663599` |
| Actual section-only diff | `/private/tmp/kkt-peach-lemur/c7/c7d-documentation.diff` | `a6ff66cd769874663a281ccf5e116479811ebf30a3f54ec1795c38f4ac1664fe` |
| Version/proof mapping | `/private/tmp/kkt-peach-lemur/c7/c7d-documentation-proof-index.json` | `10e9694dea4731553ac54d01df7a5fe3ea70c88ddf41f3bb9e5a0b319588a371` |

The new summary explains annotation editing/appearance, text/comments/fonts, page snapshots, navigation/interface fixes, and CloudPDF/delivery changes in terms of behavior and user impact. Runtime A remains the viewer-selectedf1ba46f0dc3dc5f9b055a7311dda0c13ecc0f185; optional B iseeac787c02fac7b40c74c91bbff0ffb5f59ff97e, with its separate rich-comment-default correction. No recommendation or implementation became authorized.

Removing exactly that inserted section reproduces the old document byte for byte. Every other pre-existing root C7 file stayed hash-identical; original completed handoff9daf1ff0afb2ff5b086f487280c401ea12aa32eecaa5d2011d5a5e9d41b896d2 and all53 immutable session-freeze copies remain intact. The freeze index's actual path is `session-freeze-2026-11-04/frozen-copies-index.json`; its entries point into `immutable-copies/`.

Historical C7 seals bind the original plan bytes now preserved in the old snapshot. They do not bind the changed live path; this C7d mapping explicitly records both identities. The live file's prior read-only permission was restored after its authorized in-place documentation edit. An initial write returned PermissionError before changing content; the retry enabled owner-write only for that live document and restored its original mode. This was a file-mode issue, not automatic approval rejection.

Verification used retained metadata/source-read records, local hashing and a text diff; no new repository Git/API call, clone, checkout, download, software test, build, heavy ticket or source/resource change was needed. J10 and AI4 work was untouched. Exact applied script: `python3 /private/tmp/kkt-peach-lemur/c7/c7d-update-plan.py` (final exit0); script SHA256 `87060c93123e4c63d77f6612eafb2031e0d892ba01bc056a9f4ef91b9fac5a3e`.

Next action: read the new near-top summary in the live plan; keep implementation decisions and runtime selection planned.
