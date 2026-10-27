#!/usr/bin/env python3
"""Exercise the real packager with tiny compiled inputs, without building PDFium."""

from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tempfile


APPLE = Path(__file__).resolve().parents[1]
SLICES = [
    ("ios-arm64", "arm64-apple-ios17.0", "iphoneos"),
    ("ios-sim-arm64", "arm64-apple-ios17.0-simulator", "iphonesimulator"),
    ("darwin-static-arm64", "arm64-apple-macosx14.0", "macosx"),
    ("darwin-static-x64", "x86_64-apple-macosx14.0", "macosx"),
]


with tempfile.TemporaryDirectory(prefix="embedpdf-package-test-") as temporary:
    root = Path(temporary)
    apple = root / "platforms/apple"
    build = apple / ".build"
    (build / "generated").mkdir(parents=True)
    (build / "headers").mkdir()
    (apple / "EmbedPDF").mkdir()
    (build / "generated/Smoke.swift").write_text(
        "public struct EngineSmoke { public init() {} }\n"
    )
    (build / "headers/module.modulemap").write_text('module Smoke { header "smoke.h" }\n')
    (build / "headers/smoke.h").write_text("int engine_smoke(void);\n")
    source = build / "smoke.c"
    source.write_text("int engine_smoke(void) { return 42; }\n")
    for name, triple, sdk in SLICES:
        output = build / name
        output.mkdir()
        sdk_path = subprocess.check_output(
            ["xcrun", "--sdk", sdk, "--show-sdk-path"], text=True
        ).strip()
        subprocess.run([
            "xcrun", "--sdk", sdk, "clang", "-target", triple,
            "-isysroot", sdk_path, "-c", str(source), "-o", str(output / "smoke.o"),
        ], check=True)
        subprocess.run([
            "libtool", "-static", "-o", str(output / "libEmbedPDFFFI.a"),
            str(output / "smoke.o"),
        ], check=True)
    script = apple / "package-engine-framework.sh"
    shutil.copy2(APPLE / script.name, script)
    subprocess.run(["bash", str(script)], check=True)
    framework = apple / "EmbedPDF/EmbedPDFBindings.xcframework"
    subprocess.run([
        sys.executable, str(APPLE / "tests/xcframework-layout.test.py"), str(framework),
    ], check=True)
    with (framework / "Info.plist").open("rb") as file:
        libraries = plistlib.load(file)["AvailableLibraries"]
    for library in libraries:
        platform = library["SupportedPlatform"]
        bundle = framework / library["LibraryIdentifier"] / library["LibraryPath"]
        plist = bundle / ("Resources/Info.plist" if platform == "macos" else "Info.plist")
        with plist.open("rb") as file:
            metadata = plistlib.load(file)
        key = "LSMinimumSystemVersion" if platform == "macos" else "MinimumOSVersion"
        assert metadata[key] == ("14.0" if platform == "macos" else "17.0"), metadata
    print("Packaged iOS and universal macOS frameworks with correct interfaces and OS floors")
