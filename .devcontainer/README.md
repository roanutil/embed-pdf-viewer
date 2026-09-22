# EmbedPDF development container

Open this repository with VS Code's **Dev Containers: Reopen in Container** command.
The container bind-mounts this local repository at `/workspaces/embed-pdf-viewer`;
it does not clone the repository. Edits are immediately visible on the host.
Setup also writes dependencies and generated files into this checkout.
`pnpm-workspace.yaml` retains macOS and Linux optional binaries for the current
CPU, so tools such as esbuild and Rollup work from either side of the bind mount.
After pulling this configuration into an existing checkout, run
`pnpm install --frozen-lockfile` once to install the additional platform packages.
Native addons compiled in place (such as better-sqlite3) may still require a
rebuild when switching operating systems; this setting covers optional binaries.

The existing `packages/engine/runtime/Dockerfile` builds PDFium and
`cloudpdf/server/Dockerfile` packages the server. This container supports development
of the TypeScript engine, Rust engine, geometry parity adapter, and web viewer.

## Included tools

| Tool                                                      | Version/source                                       |
| --------------------------------------------------------- | ---------------------------------------------------- |
| Node                                                      | 24.20.0                                              |
| pnpm                                                      | `package.json` `packageManager`                      |
| Rust, Cargo, rustfmt, Clippy, rust-src, cross-target std  | `rust-toolchain.toml`                                |
| Clang/libclang, LLD, CMake, Ninja, C/C++ compiler, Python | Debian Trixie packages                               |
| Emscripten / Binaryen                                     | 3.1.72 / 132; `EM_BINARYEN_ROOT` selects Binaryen    |
| Android NDK / cargo-ndk                                   | 29.0.14206865 (amd64 only) / 4.1.2                   |
| swiftly / Swift toolchain                                 | 1.1.4 / 6.3.1; `swiftly` manages further toolchains  |
| Git, Git LFS, GitHub CLI, ripgrep, jq, ShellCheck         | Debian packages                                      |
| Codex / Claude Code                                       | Latest at image build; overridable build arguments   |
| Chromium, Firefox, WebKit                                 | Workspace Playwright version; installed during setup |

Docker selects the host architecture: **linux/arm64** on Apple Silicon and ARM64
Linux, or **linux/amd64** on x86-64 hosts. Emscripten detects that architecture;
Binaryen and the native PDFium artifact are selected to match it.

## Swift

`swift` and `swiftly` are on `PATH` for the `node` user. The image pins Swift 6.3.1;
`swiftly install 6.2.1 --use`, `swiftly list-available`, and `swiftly use` switch
toolchains without a rebuild, and `swiftly` reads a `.swift-version` file if the
working directory has one. Toolchains land in `/home/node/.local/share/swiftly`,
which lives in the image rather than the bind mount, so a container rebuild discards
anything installed after the build. To change the pin permanently, edit
`SWIFT_VERSION` in `.devcontainer/Dockerfile`.

swiftly detects the host as `debian13` and refuses it, because swift.org publishes no
Trixie toolchain. The build passes `--platform debian12` to `swiftly init`, so the
Bookworm build runs against Trixie's newer glibc, and the apt list installs swiftly's
`debian12` dependency set including `libstdc++-12-dev` (12.4.0-5 in Trixie) so
`swiftly install` doesn't stop on a missing package. `swift build` and `swift test`
were verified against a scratch package on arm64 under this arrangement. `swift repl`
and `lldb` are broken: they link `libpython3.11.so.1.0`, which Trixie doesn't ship,
and fail with `error while loading shared libraries`.

The Android NDK is installed only on amd64: Google does not publish Linux ARM64
host binaries. ARM64 containers still support the TypeScript engine, native Rust
engine, WASM toolchain, and both coding CLIs. Android compilation requires an
amd64 environment; Android Studio, an emulator, and the Android application SDK
are not included. iOS compilation still requires macOS/Xcode.

First creation downloads the toolchains, installs frozen pnpm dependencies, fetches
the pinned Linux PDFium and WASM artifacts, builds the four engine packages, and
installs browsers. Allow approximately **15–30 minutes**, depending on downloads
and emulation. Later opens reuse the image and installed dependencies. Rebuild the
container after changing toolchain pins; rerun `bash .devcontainer/setup.sh` after
changing dependency or runtime pins. Missing published artifacts fail setup and
the existing fetch scripts report the expected release and recovery command.

## Use

```sh
cargo test -p epdf-engine
pnpm --filter @embedpdf/engine test
pnpm exec tsx --conditions=development tooling/engine-parity/read-geometry.ts packages/engine/main/test/fixtures/rotated_text.pdf 0
```

Run `codex` and `claude` in the container terminal to authenticate. Credentials
are not copied into the image or mounted from the host; authenticate again after
recreating the container. Both tools run as the unprivileged `node` user with their
normal approval behavior. The user has sudo for installing development packages.
Installation follows the official [Codex CLI](https://developers.openai.com/codex/cli/)
and [Claude Code npm installation](https://code.claude.com/docs/en/setup#install-with-npm)
instructions. To pin the agents, add `CODEX_VERSION` and `CLAUDE_CODE_VERSION` under
`build.args` in `devcontainer.json`.

For viewer development, bind the dev server to `0.0.0.0`; ports 3000 and 5173 are
forwarded. Viewer E2E requires the viewer build described in
[`tooling/viewer-e2e/README.md`](../tooling/viewer-e2e/README.md).

## Build without VS Code

From the repository root:

```sh
docker build -f .devcontainer/Dockerfile -t embedpdf-dev .
docker run --rm -it --init --name embed-pdf \
  --mount "type=bind,source=$(pwd),target=/workspaces/embed-pdf-viewer" \
  --workdir /workspaces/embed-pdf-viewer embedpdf-dev bash
```

Run `bash .devcontainer/setup.sh` in that shell to prepare the mounted checkout.
The Docker build context includes only container definitions and toolchain pins.
