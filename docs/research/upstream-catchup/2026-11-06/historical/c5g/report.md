Push and merge verified; original viewer target is on both branches. Full default-consumer acceptance is still failing.

| Repository | Branch | Fresh SHA |
|---|---|---|
| https://github.com/roanutil/embed-pdf-viewer | research/catchup-2026-11-combined | f28ed4ea4aa46c8e69676558993b103d68b19e08 |
| https://github.com/roanutil/embed-pdf-viewer | research/multi-platform-rust-core-impl-plugins | f28ed4ea4aa46c8e69676558993b103d68b19e08 |
| https://github.com/roanutil/embed-pdf-viewer-private | research/multi-platform-rust-core-impl-plugins | f28ed4ea4aa46c8e69676558993b103d68b19e08 |
| https://github.com/roanutil/embed-pdf-viewer-private | research/multi-platform-rust-core-impl | 3756c648aa0b9de26744f7939237903bd57a83e8 |

The public fork has no impl branch. The private impl ref was read through the existing authenticated GitHub API; the container's unauthenticated private Git request failed before comparison, then ordinary API access resolved freshness. No token/config/login changes.

Original target: plan commit 867c1c0368b8235ae6221a3d0f034591c110601e measured viewer 74afd55753ee7e6b34c57e3dc2e1a683b4c5a23b; actual Task3 merged a40d63a199f2d483fd27c9ad439f0d5efbc195aa. Both are actual ancestors of both branch tips. All nine documented impl font/header/tier followups are ancestors of both tips. Thus impl completed its original viewer merge and Task3 feature slice.

Impl is not the completed overall catch-up feature tree. Its runtime pin 8b1e69abdef4cb7c5d6c669c96a162533b2350ef includes original cff0077cff2493f89f252d1f2f039ca35b6d5c95 but does not include original D1 28b8b808f52d544b17a973199410cda0cc43530f (ancestry exit1). Plugins pin 3c55b7a1c0bf956231392a573e87f28dd6d6667d includes both (exit0). Impl's handoff explicitly defers scope permissions/thumbnail, engine font wire kinds/operation rows and vector work to C4/C5 on plugins. The JSON maps exact feature commits and changed paths for those items, page labels/import/catalog metadata, reviewed form focus/tab order/zoom/command locks/native appState, and publication/pins.

Impl is an exact ancestor of plugins: ahead 0, behind 1531, 2532 changed tree paths. These are whole-branch ancestry/tree counts, including plugin-port history and documentation, not a count of missing original-scope features. There is no squash/cherry-pick coverage ambiguity here. Later upstream main was not examined.

The pushed pin is precisely the expected two-constant Package.swift commit, parent 9deb7e29aa335403943ad298a3cb74b3937686db; blob fa6ecf1f1a1f61a915e9e5c96a280094a872625a; SHA256 67c3817e19a9d06dafcbd832c900db72da89c8fdb9d18373153d60e7a989cfc7. Runtime gitlink is 3c55b7a1c0bf956231392a573e87f28dd6d6667d. Fresh release/tag metadata ties both published prereleases to that runtime commit. Apple ZIP digest 88cfb206b6d3fd74e017062411bbd6a5133616882e52df299754aa9487c0a116 matches the manifest; WASM archive digest is 045b994b7770e39055095fb76db0d0def90b16be3b7b7f86e4fd161349bbb183. Prior immutable audit independently downloaded and hashed the bytes; its expected result SHA256 was verified here.

Material current limitation: actual default Apple consumer Xcode build at f28ed failed exit65 before tests (0 executed): Commands.swift:33, CommandHandler does not conform to CommandRunner. The source already contains locked(context:); cause remains under separate read-only diagnosis. Artifact download/checksum/11-file/Xcode binary association passed. Older override-branch 333+9 tests do not establish current default acceptance. No build or test was launched by this audit. The accepted WASM browser fixture limitation remains unchanged.

Exact command argv, exits and output hashes: fresh-metadata.json; source Git commands/results: source-audit.json, scope-audit.json and runtime-scope.json. Exact missing ancestry commits and all changed paths: source-audit.json; scoped functional mapping and prior consumer evidence: result.json.

Next action (under two minutes): read result.json verdict and originalScopeFunctionalDifferences before deciding whether impl should remain the completed Task3 layering branch or be advanced to the plugins tree; no branch change is authorized by this audit.
