#!/usr/bin/env python3
"""Synthetic orchestration tool. Never compiles or produces a native library."""
import json
import os
from pathlib import Path
import sys

root = Path(os.environ["FIXTURE_ROOT"]).resolve()
name = Path(sys.argv[0]).name
args = sys.argv[1:]
with (root / "commands.jsonl").open("a") as log:
    log.write(json.dumps({"tool": name, "argv": args, "environment": {
        key: os.environ.get(key) for key in ("EMBEDPDF_FORK_TARGET", "SDKROOT", "MACOSX_DEPLOYMENT_TARGET", "IPHONEOS_DEPLOYMENT_TARGET", "CARGO_BUILD_JOBS")}}) + "\n")

def artifact(path, content):
    path = Path(path)
    if root not in path.resolve().parents:
        raise SystemExit("fixture output escapes its temporary root")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(content)

if name == "cargo":
    target = root / "target"
    if "metadata" in args:
        print(json.dumps({"target_directory": str(target)}, separators=(",", ":")))
    elif "--target" in args:
        rust = args[args.index("--target") + 1]
        artifact(target / rust / "release/libepdf_viewer_host.a", ("FIXTURE_ARCHIVE:" + rust).encode())
        with (root / "builds.jsonl").open("a") as log:
            log.write(json.dumps({"rust": rust, "fork": os.environ.get("EMBEDPDF_FORK_TARGET"), "sdk": os.environ.get("SDKROOT"), "args": args}) + "\n")
    else:
        output = target / "debug/uniffi-bindgen"
        artifact(output, (root / "tools/bindgen.py").read_bytes())
        output.chmod(0o755)
elif name == "xcrun":
    print("/fixture-sdk/" + args[args.index("--sdk") + 1])
elif name == "nm":
    if os.environ.get("FAIL_NM") == "1":
        print("unreadable bitcode member", file=sys.stderr)
        sys.exit(1)
    print("_FPDF_InitLibraryWithConfig")
    if os.environ.get("DUPLICATE_PDFIUM") == "1":
        print("_FPDF_InitLibraryWithConfig")
    if os.environ.get("DUPLICATE_OTHER_PDFIUM") == "1":
        print("_FPDF_GetPageCount")
        print("_FPDF_GetPageCount")
elif name in ("libtool", "lipo"):
    option = "-o" if name == "libtool" else "-output"
    if option in args:
        artifact(args[args.index(option) + 1], b"FIXTURE_ARCHIVE")
elif name == "xcodebuild":
    if "-output" in args:
        Path(args[args.index("-output") + 1]).mkdir(parents=True)
    else:
        print("Xcode fixture")
else:
    raise SystemExit("unsupported synthetic tool: " + name)
