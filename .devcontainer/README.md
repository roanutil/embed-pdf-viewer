# EmbedPDF development container

Use Docker directly with any editor or terminal. The compiler toolchains, coding
agents, browser authentication, and Git credential forwarding work independently
of VS Code. An editor integration can automate setup and port forwarding; the
commands below perform those steps explicitly.

The container clones `https://github.com/roanutil/embed-pdf-viewer.git`, branch
`research/multi-platform-rust-core-impl`, into `/workspaces/embed-pdf-viewer`
inside the Docker volume `embedpdf-workspace`. The host repository is not mounted.
Dependencies, generated files, commits, and edits stay in that volume across
container rebuilds. Push commits to share them with another checkout. Removing
the volume deletes the internal checkout, including unpushed work.

The image contains the clone script, so initial creation works before the
repository exists in the volume. Existing clones are reused without pulling,
resetting, or switching branches. Pass `-e EMBEDPDF_REPOSITORY=URL` and
`-e EMBEDPDF_BRANCH=BRANCH` to `docker run` to select a different initial clone; those
settings do not change an existing clone. Container configuration and toolchain
pins used to build the image still come from the host checkout. To rebuild with
configuration changes made inside the container, push and pull them onto the host.

The existing `packages/engine/runtime/Dockerfile` builds PDFium and
`cloudpdf/server/Dockerfile` packages the server. This container supports development
of the TypeScript engine, Rust engine, geometry parity adapter, and web viewer.

## Start with Docker

Run these commands on the host from the repository root. Docker Desktop supplies
`host.docker.internal` on macOS and Windows. On Linux Docker Engine, add
`--add-host=host.docker.internal:host-gateway` to `docker run` and ensure the host
forwarders are reachable through that address; their default loopback listeners
may need a local relay.

1. Build the image:

   ```sh
   docker build -f .devcontainer/Dockerfile -t embedpdf-dev .
   ```

2. Start each host forwarder in its own terminal and leave both running:

   ```sh
   OAUTH2_FORWARDER_PORT=18080 mise exec -- o2f-server
   ```

   ```sh
   GIT_CREDENTIAL_FORWARDER_PORT=18081 mise exec -- gcf-server
   ```

3. Create a persistent container with the repository volume and local viewer ports:

   ```sh
   docker run -d --init --name embed-pdf --user node \
     --mount type=volume,source=embedpdf-workspace,target=/workspaces \
     --publish 127.0.0.1:3000:3000 \
     --publish 127.0.0.1:5173:5173 \
     --workdir /workspaces/embed-pdf-viewer embedpdf-dev
   ```

4. Clone the repository and install its dependencies:

   ```sh
   docker exec -it embed-pdf python3 /usr/local/share/embedpdf/clone.py
   docker exec -it embed-pdf python3 .devcontainer/setup.py
   ```

   First build and setup take approximately **15–30 minutes**, depending on
   downloads and emulation. The clone script preserves an existing checkout.

5. Open a development shell:

   ```sh
   docker exec -it embed-pdf bash
   ```

The Docker build context includes only container definitions and toolchain pins.
The clone and setup scripts use Python 3's standard library, with no pip packages.
Setup invokes the existing package-manager, compiler, and runtime-fetch tools.
To override pinned image tools, pass build arguments such as
`--build-arg CODEX_VERSION=VERSION`, `--build-arg CLAUDE_CODE_VERSION=VERSION`,
`--build-arg OAUTH2_FORWARDER_VERSION=VERSION`, or
`--build-arg GIT_CREDENTIAL_FORWARDER_VERSION=VERSION` to `docker build`.

## Stop, resume, and rebuild

Stop the container with `docker stop embed-pdf`. Resume it and open a shell with:

```sh
docker start embed-pdf
docker exec -it embed-pdf bash
```

The stopped container retains its home directory, authentication sessions, and
installed tools; the named volume retains the repository and dependencies.
After dependency or runtime pin changes, rerun `python3 .devcontainer/setup.py`
inside the container. Missing published artifacts fail setup; the fetch scripts
report the expected release and recovery command.

After changing image toolchain pins, rebuild the image, stop and remove the old
container, then repeat creation and setup. Removing the container discards its
home directory and authentication sessions; push work and save any files outside
`/workspaces` that you want to keep before removing it. Keep `embedpdf-workspace`
to preserve the repository.

## Editor integration

The optional `devcontainer.json` lets a compatible editor run cloning and setup
as lifecycle hooks and forward ports 3000 and 5173. Its VS Code customization
installs suggested editor extensions. Plain Docker does not read that file;
use the commands above for setup, ports, and environment overrides. See the
[Dev Container specification](https://github.com/devcontainers/spec/blob/main/docs/specs/devcontainerjson-reference.md)
for these editor and lifecycle settings.

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
| OAuth2 / Git credential forwarding clients                | oauth2-forwarder 1.9.4 / git-credential-forwarder 1.8.0 |
| Chromium, Firefox, WebKit                                 | Workspace Playwright version; installed during setup |

Docker selects the host architecture: **linux/arm64** on Apple Silicon and ARM64
Linux, or **linux/amd64** on x86-64 hosts. Emscripten detects that architecture;
Binaryen and the native PDFium artifact are selected to match it.

## Swift

`swift` and `swiftly` are on `PATH` for the `node` user. The image pins Swift 6.3.1;
`swiftly install 6.2.1 --use`, `swiftly list-available`, and `swiftly use` switch
toolchains without a rebuild, and `swiftly` reads a `.swift-version` file if the
working directory has one. Toolchains land in `/home/node/.local/share/swiftly`,
which lives in the image rather than the workspace volume, so a container rebuild discards
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

## Use

```sh
cargo test -p epdf-engine
pnpm --filter @embedpdf/engine test
pnpm exec tsx --conditions=development tooling/engine-parity/read-geometry.ts packages/engine/main/test/fixtures/rotated_text.pdf 0
```

## Host authentication forwarding

The image includes `o2f-client`, `o2f-browser`, and `gcf-client`, matching the
versions installed via mise on the host. `BROWSER=o2f-browser` forwards browser
login requests, and a system Git credential helper forwards HTTPS credentials.
These follow the [OAuth2 forwarder](https://github.com/sam-mfb/oauth2-forwarder)
and [Git credential forwarder](https://github.com/sam-mfb/git-credential-forwarder)
client setup instructions.

The startup commands above run the servers on ports 18080 and 18081. The image
connects to `host.docker.internal:18080` and `host.docker.internal:18081` by default.
For custom ports, start the host servers with the desired values and pass
`-e OAUTH2_FORWARDER_SERVER=host.docker.internal:PORT` and
`-e GIT_CREDENTIAL_FORWARDER_SERVER=host.docker.internal:PORT` to `docker run`.
No host socket mounts are required.

Run `codex` and `claude` in the container terminal to authenticate. Their local
sessions live in the container user's home and must be recreated after a rebuild.
The forwarders use the host browser and Git credential manager without copying
host credentials into the image. The `node` user has sudo for development tools.
Use the build arguments described above to pin the agents or forwarding tools.

For viewer development, bind the dev server to `0.0.0.0`; the Docker command above
publishes ports 3000 and 5173 on the host loopback interface. Viewer E2E requires the viewer build described in
[`tooling/viewer-e2e/README.md`](../tooling/viewer-e2e/README.md).
