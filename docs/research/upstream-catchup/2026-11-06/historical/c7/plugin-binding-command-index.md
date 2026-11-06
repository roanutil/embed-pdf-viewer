# C7 plugin/binding command provenance

Planning metadata only. These commands were already executed; this index does not replay them. All10 saved stdout wrappers returned0. Individual subcommand statuses are unknown; no `set -e`/`pipefail` was enabled. Existing logs and inventory remain unchanged.

| Identity | Exact SHA |
|---|---|
| reviewed_viewer_both_branches | `f28ed4ea4aa46c8e69676558993b103d68b19e08` |
| canonical_viewer_target | `4ad8986099314b9587df423c802db103a21d665d` |
| common_base | `a40d63a199f2d483fd27c9ad439f0d5efbc195aa` |
| reviewed_runtime | `3c55b7a1c0bf956231392a573e87f28dd6d6667d` |
| viewer_target_runtime_gitlink | `f1ba46f0dc3dc5f9b055a7311dda0c13ecc0f185` |
| canonical_runtime_default_head_optional | `eeac787c02fac7b40c74c91bbff0ffb5f59ff97e` |

| Actual wrapper | Exit | stdout log | SHA256 |
|---|---|---|---|
| first-delta | 0 (individual statuses unknown) | `/private/tmp/kkt-peach-lemur/c7/plugin-binding-first-delta.log` | `df04c494f55acda8fea66efa071585be0867d5f4bf9bf6f9facdcfd17e2f1856` |
| summary | 0 (individual statuses unknown) | `/private/tmp/kkt-peach-lemur/c7/plugin-binding-summary.log` | `17cdde05ba00d3e26de7b98ee10003818dfd6f321285e543e9a0572e52b31118` |
| contracts | 0 (individual statuses unknown) | `/private/tmp/kkt-peach-lemur/c7/plugin-binding-contracts.log` | `bd38ce3d8fdde847150e7e8c8f0eb72c3b16fbe26dba4ea5b08363a9a72fe2b4` |
| native-contract-proof | 0 (individual statuses unknown) | `/private/tmp/kkt-peach-lemur/c7/plugin-binding-native-contract-proof.log` | `a747f785b253821e956edb00cea9f6c7e62f897064a8934bbd6a980708bd97f9` |
| payload-proof | 0 (individual statuses unknown) | `/private/tmp/kkt-peach-lemur/c7/plugin-binding-payload-proof.log` | `cf2147731b899229d4e18399b250ff05e5fd73c2e8c5be249e1e9a0ea38cd306` |
| baseline-surfaces | 0 (individual statuses unknown) | `/private/tmp/kkt-peach-lemur/c7/plugin-binding-baseline-surfaces.log` | `fbadbc010ed075098f6ec95e098d8e9b7f1528c49e91e2e06517966f283fc8b3` |
| preservation-proof | 0 (individual statuses unknown) | `/private/tmp/kkt-peach-lemur/c7/plugin-binding-preservation-proof.log` | `9978bb2d90e0bad4721a9cd1927f50a6a7cd95778b968e0f3004128054afbf7b` |
| detail-proof | 0 (individual statuses unknown) | `/private/tmp/kkt-peach-lemur/c7/plugin-binding-detail-proof.log` | `69753b0b211c947b47ad48394dc632df86bc72f8205ea88cee842b8cf773cdf7` |
| package-proof | 0 (individual statuses unknown) | `/private/tmp/kkt-peach-lemur/c7/plugin-binding-package-proof.log` | `fc37be5b488063ed36ac43201ac99927e9ebc66586c5f4b1fd5bd1262bf780ab` |
| exact-commits | 0 (individual statuses unknown) | `/private/tmp/kkt-peach-lemur/c7/plugin-binding-exact-commits.log` | `c2fbc853bf36bf800b293f878f406daeceac6f1008a9392ecf49670b97bedb7a` |

Host cwd: `/Users/andrew/repos/embed-pdf-viewer` inherited; no explicit workdir argument. Container default cwd: `/workspaces/embed-pdf-viewer`. Wrapper argv for every entry: `docker exec embedpdf-c5-combined bash -c <body>`, host stdout redirected to listed file. Actual shell bodies and full host commands follow.

**Limitations:** `head` truncates output and may mask upstream errors; `rg` no-match/errors are not proof of absence beyond the named search. Preservation read emitted missing-path errors; payload/detail searches suppressed stderr on exploratory paths. The output log captures stdout only; observed preservation stderr is recorded in JSON. No individual command is reported as exit0 without evidence.

## first-delta

```sh
docker exec embedpdf-c5-combined bash -c 'git --git-dir=/tmp/embedpdf-c7-viewer-metadata.git log --oneline refs/c7/reviewed..refs/c7/upstream; git --git-dir=/tmp/embedpdf-c7-viewer-metadata.git diff --stat refs/c7/reviewed...refs/c7/upstream; git --git-dir=/tmp/embedpdf-c7-viewer-metadata.git diff --name-status refs/c7/reviewed...refs/c7/upstream' > /private/tmp/kkt-peach-lemur/c7/plugin-binding-first-delta.log
```

## summary

```sh
docker exec embedpdf-c5-combined bash -c 'git --git-dir=/tmp/embedpdf-c7-viewer-metadata.git diff refs/c7/reviewed...refs/c7/upstream -- packages/plugin packages/core/annotation packages/core/fonts packages/core/stage packages/core/ui packages/engine/core/src/dto platforms crates > /tmp/c7-plugin-binding.patch; git --git-dir=/tmp/embedpdf-c7-viewer-metadata.git diff --name-only refs/c7/reviewed...refs/c7/upstream -- crates platforms; git --git-dir=/tmp/embedpdf-c7-viewer-metadata.git merge-base refs/c7/reviewed refs/c7/upstream; git --git-dir=/tmp/embedpdf-c7-viewer-metadata.git diff --numstat refs/c7/reviewed...refs/c7/upstream -- packages/plugin; git --git-dir=/tmp/embedpdf-c7-viewer-metadata.git log --oneline refs/c7/reviewed..refs/c7/upstream -- packages/plugin packages/core/annotation packages/core/fonts packages/core/ui' > /private/tmp/kkt-peach-lemur/c7/plugin-binding-summary.log
```

## contracts

```sh
docker exec embedpdf-c5-combined bash -c 'git --git-dir=/tmp/embedpdf-c7-viewer-metadata.git diff refs/c7/reviewed refs/c7/upstream -- packages/plugin/annotation/src/contract.ts packages/plugin/annotation/src/host-contract.ts packages/plugin/render/src/contract.ts packages/plugin/stage/src/contract.ts packages/plugin/commands/src/standard.ts; git --git-dir=/tmp/embedpdf-c7-viewer-metadata.git log --oneline -12 refs/c7/reviewed; git --git-dir=/tmp/embedpdf-c7-viewer-metadata.git ls-tree -r --name-only refs/c7/reviewed crates/plugins platforms | head -70' > /private/tmp/kkt-peach-lemur/c7/plugin-binding-contracts.log
```

## native-contract-proof

```sh
docker exec embedpdf-c5-combined bash -c 'cd /workspaces/embed-pdf-viewer && rg -n "Snapshot|snapshot|draft_rich_text|typing_style|list_current_pages|turn_gestures|can_snapshot|copy_image|standard_fallback|fallback_standard" crates/plugins/{render,stage,annotation,fonts,commands} crates/epdf-{uniffi,ffi,capi,wasm,web} platforms/{apple,android,dotnet} --glob "!Cargo.lock" --glob "!*.api.*" | head -130; git --git-dir=/tmp/embedpdf-c7-viewer-metadata.git diff --name-only refs/c7/reviewed refs/c7/upstream -- packages/plugin; git --git-dir=/tmp/embedpdf-c7-viewer-metadata.git diff refs/c7/reviewed refs/c7/upstream -- packages/engine/core/src/dto/DocumentFonts.ts packages/engine/core/src/annotation/base.schema.ts packages/core/annotation/src packages/core/fonts/src | head -380' > /private/tmp/kkt-peach-lemur/c7/plugin-binding-native-contract-proof.log
```

## payload-proof

```sh
docker exec embedpdf-c5-combined bash -c 'cd /workspaces/embed-pdf-viewer && rg -n "draft_rich_text|struct RenderPageOptions|struct StageSettings|list_current_item_pages|struct StandardCommandsPlatform|trait StandardCommandsPlatform|pub fn.*font|pub.*color|pub.*dash" crates/plugins/{render,stage,annotation,fonts,commands}/src/{contract.rs,host_contract.rs,ffi.rs,model.rs,settings.rs,rich_text.rs} 2>/dev/null; ls crates/plugins/{render,stage,annotation,fonts,commands}/src; rg -n "draftRichText|renderPage|nextPage|snapshot|toggleFormat|color|dash" platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins platforms/dotnet/EmbedPDF/Plugins platforms/android/embedpdf/library/src/main/kotlin/com/embedpdf 2>/dev/null | head -65; git --git-dir=/tmp/embedpdf-c7-viewer-metadata.git diff refs/c7/reviewed refs/c7/upstream -- packages/engine/core/src/dto/DocumentFonts.ts packages/core/fonts/src/font-plan.ts packages/engine/core/src/annotation/base.schema.ts packages/plugin/fonts/src/controller.ts packages/plugin/fonts/src/paint.ts' > /private/tmp/kkt-peach-lemur/c7/plugin-binding-payload-proof.log
```

## baseline-surfaces

```sh
docker exec embedpdf-c5-combined bash -c 'cd /workspaces/embed-pdf-viewer && sed -n "145,235p" crates/plugins/render/src/contract.rs; sed -n "60,112p" crates/plugins/render/src/ffi.rs; sed -n "177,232p" crates/plugins/stage/src/contract.rs; rg -n "draft_rich_text|toggle_format|set_text_selection" crates/plugins/annotation/src/{write,ffi,harness}; rg -n "pub fn|fallback|standard_font" crates/plugins/fonts/src/{lib,font,css}.rs | head -50; rg --files platforms | rg "(Plugins/Annotation|Plugins/Stage|Plugins/Render|plugin|Plugin|Parity|Contract|generate|pack|release)" | head -70; cat crates/plugins/{render,stage,annotation,commands,annotation-core,fonts}/Cargo.toml | rg "^name|^\[|wasm|uniffi"; cat package.json | sed -n "1,120p"' > /private/tmp/kkt-peach-lemur/c7/plugin-binding-baseline-surfaces.log
```

## preservation-proof

```sh
docker exec embedpdf-c5-combined bash -c 'git --git-dir=/tmp/embedpdf-c7-viewer-metadata.git log --format="%H %s" refs/c7/upstream..refs/c7/reviewed -- packages/plugin packages/engine/core/src/pageSpace/helpers.ts packages/engine/services/src/features/destinations/readDestination.ts; git --git-dir=/tmp/embedpdf-c7-viewer-metadata.git diff refs/c7/reviewed refs/c7/upstream -- packages/plugin/actions/src/lifecycle/document-events.ts packages/plugin/actions/test/script-print-policy.test.ts packages/plugin/search/test/search.test.ts packages/engine/services/src/features/destinations/readDestination.ts packages/engine/core/src/pageSpace/helpers.ts; cd /workspaces/embed-pdf-viewer && sed -n "155,185p" crates/plugins/annotation/src/write/text_editing.rs; rg -n "kind.*(cannot|immutable|change)|siblings|dashArray|dash_array|color.*null" crates/plugins/annotation-core/src/{props.rs,record,scene} crates/epdf-engine/src/features/annotations --glob "*.rs" | head -50; ls scripts | rg "(plugin|parity|bind|c5|generate|native)"; ls tooling | rg "(plugin|parity)"; cat platforms/android/embedpdf/library/build.gradle.kts | head -35; cat platforms/apple/EmbedPDF/Package.swift | head -60; cat platforms/dotnet/EmbedPDF/EmbedPDF.csproj | head -50' > /private/tmp/kkt-peach-lemur/c7/plugin-binding-preservation-proof.log
```

## detail-proof

```sh
docker exec embedpdf-c5-combined bash -c 'cd /workspaces/embed-pdf-viewer && rg --files crates/plugins/annotation-core/src | head -25; rg -n "pub struct.*(Patch|Annotation)|dash_array|dashArray|RichText|comment" crates/epdf-engine/src/annotation crates/epdf-capi/src platforms/dotnet/EmbedPDF/Models.cs 2>/dev/null | head -65; ls tooling/plugin-conformance; ls tooling/dotnet-parity; ls platforms/android; ls platforms/apple | head -20; git --git-dir=/tmp/embedpdf-c7-viewer-metadata.git diff refs/c7/reviewed refs/c7/upstream -- packages/plugin/annotation/src/read/properties.ts packages/plugin/annotation/src/read/text-fonts.ts packages/plugin/annotation/src/write/selection.ts packages/plugin/render/src/host-contract.ts packages/engine/core/src/annotation/kinds/shared-fields.ts; git --git-dir=/tmp/embedpdf-c7-viewer-metadata.git diff --name-only refs/c7/reviewed...refs/c7/upstream | python3 -c "import sys,collections; x=[s.strip() for s in sys.stdin]; print(len(x)); print(collections.Counter(\"/\".join(p.split(\"/\")[:3]) for p in x if p.startswith(\"packages/plugin/\") and \"/src/\" in p)); print(\"native paths\",sum(p.startswith((\"crates/\",\"platforms/\")) for p in x))"' > /private/tmp/kkt-peach-lemur/c7/plugin-binding-detail-proof.log
```

## package-proof

```sh
docker exec embedpdf-c5-combined bash -c 'cd /workspaces/embed-pdf-viewer && cat tooling/plugin-conformance/package.json; cat tooling/dotnet-parity/README.md | head -90; sed -n "1,125p" platforms/dotnet/EmbedPDF/Models.cs; sed -n "1,100p" crates/plugins/annotation/src/ffi/drafts.rs; sed -n "1,90p" platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Annotation.swift; ls crates/epdf-engine/src; git --git-dir=/tmp/embedpdf-c7-viewer-metadata.git diff refs/c7/reviewed...refs/c7/upstream -- .github/workflows/release.yml packages/engine/runtime/engine-runtime-build.json packages/plugin/commands/package.json .changeset/config.json' > /private/tmp/kkt-peach-lemur/c7/plugin-binding-package-proof.log
```

## exact-commits

```sh
docker exec embedpdf-c5-combined bash -c 'cd /workspaces/embed-pdf-viewer && cat crates/plugins/annotation-core/Cargo.toml | head -20; cat crates/plugins/fonts/Cargo.toml | head -15; sed -n "1,105p" platforms/android/README.md; rg --files crates/epdf-engine/src/features | rg "annot" | head -15; rg --files crates/plugins | rg "(font.*controller|text_fonts|text-fonts|standard.*commands|standard_commands)"; ls tooling/plugin-conformance/src; git --git-dir=/tmp/embedpdf-c7-viewer-metadata.git log --format="%H %s" refs/c7/reviewed..refs/c7/upstream -- packages/plugin packages/core/annotation packages/core/fonts packages/core/ui; git --git-dir=/tmp/embedpdf-c7-viewer-metadata.git diff refs/c7/reviewed refs/c7/upstream -- packages/engine/runtime/engine-runtime-build.json | head -18' > /private/tmp/kkt-peach-lemur/c7/plugin-binding-exact-commits.log
```

Font clarification from lead: final display-chain fallback mapping may remain shared TS after merge; trace/validate the existing owner, rather than creating an assumed Rust mapper. Runtime fallback remains new.
