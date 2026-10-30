# Development Container .NET Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build and run .NET 10 native interop programs in the project container.

**Architecture:** Add an integrity-checked SDK archive installation to the existing
multiarchitecture Dockerfile. Keep installation declarative and validation in
new disposable containers.

**Tech Stack:** Docker BuildKit, Debian Trixie, .NET SDK 10.0.403, Clang.

**Spec:** `docs/superpowers/specs/2026-10-30-devcontainer-dotnet-design.md`

## Global Constraints

Exact SDK version and per-architecture SHA512; amd64 and arm64; developer user
`node`; no manual installation into running containers or release publication.

## Review Focus

SDK lookup in login shells; wrong archive architecture; integrity failure before
extraction; missing native runtime libraries; native interop as the non-root user.

### Task 1: Install and verify the SDK

**Files:** Modify `.devcontainer/Dockerfile` and `.devcontainer/README.md`.

**Interfaces:** Microsoft's pinned Linux archives produce `/opt/dotnet/dotnet`,
available as `dotnet` to the development user.

- [x] Verify baseline: running project container has no `dotnet`, but libclang
  and runtime dependencies are present. Cross-check official SDK hashes.
- [x] Add architecture selection, checksum verification before extraction,
  SDK environment/command setup and a build-time exact-version assertion.
- [x] Document installation, pin updates and managed build usage.
- [x] Build the actual Dockerfile for linux/arm64 and linux/amd64. Run scratch
  net10.0 console + LibraryImport/C-library smoke programs as `node`, check
  libclang loading and login/non-login SDK selection. Record image IDs and
  whether validation was emulated. Report limitations without claiming parity.
- [ ] Review the diff, commit the isolated branch and open a PR for human review.

## Execution record

Baseline: Linux aarch64 project container, image
`sha256:5a9660df011c9735036a9de05592c377fcbc9d3222ea3561c28c77710b7d98a7`;
Rust 1.98.0, Node 24.20.0, libclang 19, no .NET SDK. Implementation follows the
approved declarative toolchain slice. Archive layout and smoke checks are
implementation choices. Existing runtime packages suffice, avoiding an unrelated
apt-layer rebuild.

Validation: both complete images built successfully and passed SDK selection in
login/non-login shells, libclang loading, net10.0 restore/build, a C function
called through `LibraryImport`, and ICU formatting as `node`. Linux arm64 ran
natively; Linux amd64 ran under Docker emulation on macOS arm64. Build output
had zero compiler warnings/errors. A deliberately incorrect ARM64 SHA512 caused
the build to fail at checksum verification, before extraction.

- arm64 image: `sha256:edfb3af3c66b0c9fa51322b39e524d3996d30da7e25d61edab8e9a13508e4703`
- amd64 image: `sha256:59ee6d065d3779c927d141cf19cfcf394238326c58f307318f02c04edc1789cf`

Independent peer review found no blocking findings. The SDK emits its known
first-run workload-verification advisory; no optional workloads are installed,
and the required builds/runs succeed. Integrity checks are not suppressed.
Changeset status against the fork's research branch reports no package bumps;
only development tooling and documentation are affected.
