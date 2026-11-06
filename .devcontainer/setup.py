#!/usr/bin/env python3
"""Install dependencies and prepare the TypeScript and Rust development tools."""

import os
from pathlib import Path
import subprocess
import sys


def run(*command, check=True):
    return subprocess.run(command, check=check)


def main():
    os.chdir(Path(__file__).resolve().parent.parent)
    # Public dependencies need no publishing token; preserve one if supplied.
    os.environ.setdefault("NPM_TOKEN", "")
    subprocess.run(
        ["pnpm", "install", "--frozen-lockfile"],
        env={**os.environ, "CI": "true"},
        check=True,
    )

    # Rust links the pinned fork; the TypeScript reference needs its WASM payload.
    runtime_target = subprocess.check_output(
        ["node", "-p", "'linux-' + process.arch"], text=True
    ).strip()
    run("bash", "packages/engine/runtime/scripts/fetch-libpdfium.sh", runtime_target)

    # This artifact is still pending in the fork, matching build/test-all.sh.
    pending = run(
        "bash", "packages/engine/runtime/scripts/fetch-libpdfium.sh", "wasm32-eh",
        check=False,
    )
    if pending.returncode:
        print("skipped: wasm32-eh is pending in engine-runtime-build.json", file=sys.stderr)

    run("bash", "packages/engine/runtime/scripts/fetch-payload.sh", "wasm32")
    for package in (
        "@embedpdf/engine-runtime",
        "@embedpdf/engine-core",
        "@embedpdf/engine-services",
        "@embedpdf/engine",
    ):
        run("pnpm", "--filter", package, "build")

    # Browser versions follow the workspace's Playwright dependency.
    run(
        "pnpm", "--filter", "@embedpdf/tooling-viewer-e2e", "exec", "playwright",
        "install", "--with-deps", "chromium", "firefox", "webkit",
    )
    run("cargo", "fetch", "--locked")
    print("\nReady: cargo test -p epdf-engine; pnpm --filter @embedpdf/engine test")


if __name__ == "__main__":
    main()
