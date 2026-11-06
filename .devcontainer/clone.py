#!/usr/bin/env python3
"""Clone the development repository once, preserving work across rebuilds."""

import os
from pathlib import Path
import subprocess


def main():
    workspace = Path(os.environ.get("EMBEDPDF_WORKSPACE") or "/workspaces/embed-pdf-viewer")
    repository = (
        os.environ.get("EMBEDPDF_REPOSITORY")
        or "https://github.com/roanutil/embed-pdf-viewer.git"
    )
    branch = os.environ.get("EMBEDPDF_BRANCH") or "research/multi-platform-rust-core-impl"

    if (workspace / ".git").is_dir():
        print(f"Reusing repository at {workspace}")
        return

    subprocess.run(
        ["git", "clone", "--branch", branch, "--", repository, str(workspace)],
        check=True,
    )


if __name__ == "__main__":
    main()
