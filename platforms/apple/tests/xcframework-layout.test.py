#!/usr/bin/env python3
"""Validate the shipping platform/architecture contract of either Apple engine."""

import pathlib
import plistlib
import subprocess
import sys
import unittest


FRAMEWORK = pathlib.Path(sys.argv.pop(1)).resolve()


class XCFrameworkLayoutTests(unittest.TestCase):
    def test_supported_platforms_and_architectures(self):
        with (FRAMEWORK / "Info.plist").open("rb") as file:
            libraries = plistlib.load(file)["AvailableLibraries"]
        actual = {
            (lib["SupportedPlatform"], lib.get("SupportedPlatformVariant", "")):
                set(lib["SupportedArchitectures"])
            for lib in libraries
        }
        self.assertEqual(len(libraries), 3)
        self.assertEqual(actual, {
            ("ios", ""): {"arm64"},
            ("ios", "simulator"): {"arm64"},
            ("macos", ""): {"arm64", "x86_64"},
        })
        for lib in libraries:
            with self.subTest(library=lib["LibraryIdentifier"]):
                directory = FRAMEWORK / lib["LibraryIdentifier"]
                library = directory / lib["LibraryPath"]
                if library.suffix == ".framework":
                    binary = library / library.stem
                    module = library / "Modules" / "EmbedPDFBindings.swiftmodule"
                    platform = lib["SupportedPlatform"]
                    if platform == "macos":
                        self.assertEqual((library / "Versions/Current").readlink(), pathlib.Path("A"))
                        for link in ["Modules", "Resources", "EmbedPDFBindings"]:
                            self.assertTrue((library / link).is_symlink(), str(library / link))
                    else:
                        self.assertFalse((library / "Versions").exists())
                    plist = library / ("Resources/Info.plist" if platform == "macos" else "Info.plist")
                    with plist.open("rb") as file:
                        metadata = plistlib.load(file)
                    key = "LSMinimumSystemVersion" if platform == "macos" else "MinimumOSVersion"
                    self.assertEqual(metadata[key], "14.0" if platform == "macos" else "17.0")
                    suffix = "-simulator" if lib.get("SupportedPlatformVariant") else ""
                    for arch in lib["SupportedArchitectures"]:
                        interface = module / f"{arch}-apple-{platform}{suffix}.swiftinterface"
                        self.assertTrue(interface.is_file(), str(interface))
                        self.assertNotIn("FFI", interface.read_text())
                else:
                    binary = library
                    self.assertTrue((directory / lib["HeadersPath"] / "module.modulemap").is_file())
                arches = subprocess.check_output(["lipo", "-archs", str(binary)], text=True).split()
                self.assertEqual(set(arches), set(lib["SupportedArchitectures"]))


if __name__ == "__main__":
    unittest.main()
