"""Contracts for source-pinned native archives and generated interfaces."""
import hashlib
import importlib.util
import json
import os
import plistlib
import zipfile
import pathlib
import struct
import subprocess
import sys
import tempfile
import unittest

VERIFIER = pathlib.Path(__file__).resolve().parents[1] / "check-native-graph.py"
VIEWER = "1" * 40
RUNTIME = "2" * 40
BASE = "3c55b7a1c0bf956231392a573e87f28dd6d6667d"
TARGETS = {
    "ios-arm64": ("aarch64-apple-ios", "arm64-apple-ios17.0", "iphoneos", "17.0", 0x100000c, 2),
    "ios-sim-arm64": ("aarch64-apple-ios-sim", "arm64-apple-ios17.0-simulator", "iphonesimulator", "17.0", 0x100000c, 7),
    "darwin-static-arm64": ("aarch64-apple-darwin", "arm64-apple-macosx14.0", "macosx", "14.0", 0x100000c, 1),
    "darwin-static-x64": ("x86_64-apple-darwin", "x86_64-apple-macosx14.0", "macosx", "14.0", 0x1000007, 1),
}

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def archive(cpu, platform, minimum):
    major = int(minimum.split(".")[0])
    command = struct.pack("<6I", 0x32, 24, platform, major << 16, 27 << 16, 0)
    obj = struct.pack("<8I", 0xfeedfacf, cpu, 0, 1, 1, len(command), 0, 0) + command
    header = f"fixture.o/      {0:<12}{0:<6}{0:<6}{'100644':<8}{len(obj):<10}`\n".encode()
    assert len(header) == 60
    return b"!<arch>\n" + header + obj + (b"\n" if len(obj) % 2 else b"")

class NativeGraphTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = pathlib.Path(self.temp.name)
        self.receipt = {"schemaVersion": 1, "viewerCommit": VIEWER, "runtimeCommit": RUNTIME,
                        "runtimeBase": BASE, "nativeSourceHash": "a" * 64,
                        "cargoLockSha256": "b" * 64, "toolInputInventorySha256": "c" * 64,
                        "uniffiVersion": "0.32.0", "targets": {}}
        for target, (rust, swift, sdk, minimum, cpu, platform) in TARGETS.items():
            root = self.root / target
            (root / "lib").mkdir(parents=True)
            (root / "include").mkdir()
            (root / "include/fixture.h").write_text("void EPDF_Test(void);\n")
            (root / "lib/libembedpdf.a").write_bytes(archive(cpu, platform, minimum))
            (root / "BUILD-METADATA.json").write_text(json.dumps({"sha": RUNTIME, "target": target}))
            gn = ["pdf_is_complete_lib=true", "embedpdf_thread_local_globals=true", "pdf_enable_brotli=false"]
            gn += [f'mac_deployment_target="{minimum}"'] if sdk == "macosx" else [f'ios_deployment_target="{minimum}"']
            (root / "args.gn").write_text("\n".join(gn) + "\n")
            generated = root / "generated"
            generated.mkdir()
            (generated / "epdf.swift").write_text("if uniffi_epdf_checksum_test() != 123 { fatalError() }\n")
            (generated / "epdfFFI.h").write_text("unsigned short uniffi_epdf_checksum_test(void);\n")
            (generated / "epdfFFI.modulemap").write_text('module epdfFFI { header "epdfFFI.h" export * }\n')
            self.receipt["targets"][target] = {"rustTarget": rust, "swiftTarget": swift, "sdk": sdk,
                "minimumOS": minimum, "runtimeRoot": str(root),
                "runtimeArchiveSha256": digest(root / "lib/libembedpdf.a"),
                "gnArgsSha256": digest(root / "args.gn"),
                "headerHashes": {"fixture.h": digest(root / "include/fixture.h")},
                "rustArchive": str(root / "lib/libembedpdf.a"),
                "rustArchiveSha256": digest(root / "lib/libembedpdf.a"),
                "generatedRoot": str(generated),
                "generatedHashes": {p.name: digest(p) for p in generated.iterdir()}}

    def run_graph(self, phase="runtime", **environment):
        path = self.root / "receipt.json"
        path.write_text(json.dumps(self.receipt))
        return subprocess.run([sys.executable, str(VERIFIER), "--phase", phase, "--receipt", str(path),
                               "--expected-viewer", VIEWER, "--expected-runtime", RUNTIME], text=True, capture_output=True, env=dict(os.environ, **environment))

    def rejects(self, message, phase="runtime"):
        result = self.run_graph(phase)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(message, result.stderr)

    def test_valid_source_pinned_four_target_runtime(self):
        result = self.run_graph()
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_old_runtime_source_cannot_be_relabelled(self):
        self.receipt["runtimeCommit"] = BASE
        self.rejects("runtimeCommit")

    def test_runtime_archive_corruption_is_rejected(self):
        root = pathlib.Path(self.receipt["targets"]["darwin-static-x64"]["runtimeRoot"])
        (root / "lib/libembedpdf.a").write_bytes(b"stale")
        self.rejects("runtimeArchiveSha256")

    def test_a_dylib_cannot_masquerade_as_static_archive(self):
        row = self.receipt["targets"]["ios-arm64"]
        path = pathlib.Path(row["runtimeRoot"]) / "lib/libembedpdf.a"
        path.write_bytes(struct.pack("<8I", 0xfeedfacf, 0x100000c, 0, 6, 0, 0, 0, 0))
        row["runtimeArchiveSha256"] = digest(path)
        self.rejects("static archive")

    def test_wrong_macos_architecture_is_rejected(self):
        row = self.receipt["targets"]["darwin-static-x64"]
        path = pathlib.Path(row["runtimeRoot"]) / "lib/libembedpdf.a"
        path.write_bytes(archive(0x100000c, 1, "14.0"))
        row["runtimeArchiveSha256"] = digest(path)
        self.rejects("architecture")

    def test_a_newer_macos_floor_is_rejected(self):
        row = self.receipt["targets"]["darwin-static-x64"]
        path = pathlib.Path(row["runtimeRoot"]) / "lib/libembedpdf.a"
        path.write_bytes(archive(0x1000007, 1, "15.0"))
        row["runtimeArchiveSha256"] = digest(path)
        self.rejects("deployment")

    def test_simulator_cannot_use_device_platform(self):
        row = self.receipt["targets"]["ios-sim-arm64"]
        path = pathlib.Path(row["runtimeRoot"]) / "lib/libembedpdf.a"
        path.write_bytes(archive(0x100000c, 2, "17.0"))
        row["runtimeArchiveSha256"] = digest(path)
        self.rejects("platform")

    def test_header_corruption_is_rejected(self):
        row = self.receipt["targets"]["ios-arm64"]
        (pathlib.Path(row["runtimeRoot"]) / "include/fixture.h").write_text("old API")
        self.rejects("headerHashes")

    def test_missing_architecture_is_rejected(self):
        del self.receipt["targets"]["darwin-static-x64"]
        self.rejects("targets")

    def test_matching_generated_interfaces_pass(self):
        result = self.run_graph("bindings")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_matching_hashes_do_not_hide_cross_slice_api_skew(self):
        row = self.receipt["targets"]["darwin-static-x64"]
        path = pathlib.Path(row["generatedRoot"]) / "epdf.swift"
        path.write_text("if uniffi_epdf_checksum_test() != 124 { fatalError() }\n")
        row["generatedHashes"]["epdf.swift"] = digest(path)
        self.rejects("generated API", "bindings")

    def test_generated_interface_inventory_rejects_missing_headers(self):
        row = self.receipt["targets"]["ios-arm64"]
        path = pathlib.Path(row["generatedRoot"]) / "epdfFFI.h"
        path.unlink()
        del row["generatedHashes"]["epdfFFI.h"]
        self.rejects("generated", "bindings")

    def test_bindings_reject_hash_matching_rust_archive_above_target_minimum(self):
        row = self.receipt["targets"]["darwin-static-arm64"]
        path = self.root / "rust-macos15.a"
        path.write_bytes(archive(0x100000c, 1, "15.0"))
        row["rustArchive"] = str(path)
        row["rustArchiveSha256"] = digest(path)
        self.rejects("Rust archive deployment exceeds 14.0", "bindings")

    def test_rust_archive_cannot_be_substituted_after_bindings_generation(self):
        row = self.receipt["targets"]["ios-arm64"]
        row["rustArchiveSha256"] = "f" * 64
        self.rejects("rustArchiveSha256", "bindings")

    def package_fixture(self):
        root = self.root / "EmbedPDFBindings.xcframework"
        root.mkdir()
        entries = []
        for identifier, platform, variant, architectures, cpus, minimum in [
            ("ios-arm64", "ios", "", ["arm64"], [0x100000c], "17.0"),
            ("ios-arm64-simulator", "ios", "simulator", ["arm64"], [0x100000c], "17.0"),
            ("macos-arm64_x86_64", "macos", "", ["arm64", "x86_64"], [0x100000c, 0x1000007], "14.0"),
        ]:
            framework = root / identifier / "EmbedPDFBindings.framework"
            modules = framework / "Modules/EmbedPDFBindings.swiftmodule"
            modules.mkdir(parents=True)
            for architecture in architectures:
                suffix = "-simulator" if variant else ""
                (modules / f"{architecture}-apple-{platform}{suffix}.swiftinterface").write_text("public struct Viewer {}\n")
            native_platform = 1 if platform == "macos" else 7 if variant else 2
            archives = [archive(cpu, native_platform, minimum) for cpu in cpus]
            if len(archives) == 1:
                binary = archives[0]
            else:
                offset = 8 + 20 * len(archives)
                headers = []
                for cpu, data in zip(cpus, archives):
                    headers.append(struct.pack(">5I", cpu, 0, offset, len(data), 0))
                    offset += len(data)
                binary = struct.pack(">2I", 0xcafebabe, 2) + b"".join(headers) + b"".join(archives)
            (framework / "EmbedPDFBindings").write_bytes(binary)
            plist = framework / ("Resources/Info.plist" if platform == "macos" else "Info.plist")
            plist.parent.mkdir(exist_ok=True)
            plist.write_bytes(plistlib.dumps({"LSMinimumSystemVersion" if platform == "macos" else "MinimumOSVersion": minimum}))
            entry = {"LibraryIdentifier": identifier, "LibraryPath": "EmbedPDFBindings.framework",
                     "SupportedPlatform": platform, "SupportedArchitectures": architectures}
            if variant:
                entry["SupportedPlatformVariant"] = variant
            if platform == "macos":
                version = framework / "Versions/A"
                version.mkdir(parents=True)
                for name in ["Modules", "Resources", "EmbedPDFBindings"]:
                    (framework / name).rename(version / name)
                    (framework / name).symlink_to("Versions/Current/" + name)
                (framework / "Versions/Current").symlink_to("A")
            entries.append(entry)
        (root / "Info.plist").write_bytes(plistlib.dumps({"AvailableLibraries": entries}))
        self.receipt["xcframeworkRoot"] = str(root)
        self.refresh_package()
        return root

    def refresh_package(self):
        root = pathlib.Path(self.receipt["xcframeworkRoot"])
        self.receipt["xcframeworkHashes"] = {str(p.relative_to(root)): {"symlink": os.readlink(p)} if p.is_symlink() else digest(p) for p in root.rglob("*") if p.is_symlink() or p.is_file()}
        zip_path = self.root / "EmbedPDFBindings.xcframework.zip"
        with zipfile.ZipFile(zip_path, "w") as bundle:
            for path in root.rglob("*"):
                if path.is_symlink():
                    entry = zipfile.ZipInfo(str(path.relative_to(root.parent)))
                    entry.create_system = 3
                    entry.external_attr = (0o120777 << 16)
                    bundle.writestr(entry, os.readlink(path))
                elif path.is_file():
                    bundle.write(path, str(path.relative_to(root.parent)))
        self.receipt["zipPath"] = str(zip_path)
        self.receipt["zipSha256"] = digest(zip_path)

    def test_three_entry_universal_framework_and_exact_zip_pass(self):
        self.package_fixture()
        result = self.run_graph("xcframework")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_missing_macos_interface_is_rejected_even_with_updated_hashes(self):
        root = self.package_fixture()
        next(root.glob("macos*/**/x86_64*.swiftinterface")).unlink()
        self.refresh_package()
        self.rejects("interface", "xcframework")

    def test_duplicate_platform_variant_is_rejected(self):
        root = self.package_fixture()
        path = root / "Info.plist"
        metadata = plistlib.loads(path.read_bytes())
        metadata["AvailableLibraries"][1]["SupportedPlatformVariant"] = ""
        path.write_bytes(plistlib.dumps(metadata))
        self.refresh_package()
        self.rejects("platform", "xcframework")

    def test_plist_cannot_claim_a_higher_macos_minimum(self):
        root = self.package_fixture()
        path = next(root.glob("macos*/**/Info.plist"))
        path.write_bytes(plistlib.dumps({"LSMinimumSystemVersion": "15.0"}))
        self.refresh_package()
        self.rejects("minimum", "xcframework")

    def test_interfaces_must_not_leak_c_ffi_modules(self):
        root = self.package_fixture()
        path = next(root.glob("macos*/**/arm64*.swiftinterface"))
        path.write_text("import epdfFFI\n")
        self.refresh_package()
        self.rejects("FFI", "xcframework")

    def test_zip_must_match_the_tested_extracted_framework(self):
        self.package_fixture()
        with zipfile.ZipFile(self.receipt["zipPath"], "a") as bundle:
            bundle.writestr("EmbedPDFBindings.xcframework/extra", "unreviewed")
        self.receipt["zipSha256"] = digest(pathlib.Path(self.receipt["zipPath"]))
        self.rejects("ZIP", "xcframework")

    def test_macos_framework_must_keep_versioned_internal_links(self):
        root = self.package_fixture()
        framework = root / "macos-arm64_x86_64/EmbedPDFBindings.framework"
        link = framework / "Versions/Current"
        link.unlink()
        # This alias remains readable but does not follow the versioned contract.
        link.symlink_to("../Versions/A")
        self.refresh_package()
        self.rejects("versioned", "xcframework")

    def bitcode_fixture(self):
        import shutil
        row = self.receipt["targets"]["ios-arm64"]
        path = pathlib.Path(row["runtimeRoot"]) / "lib/libembedpdf.a"
        obj = b"BC\xc0\xdefixture"
        header = f"bitcode.o/      {0:<12}{0:<6}{0:<6}{'100644':<8}{len(obj):<10}`\n".encode()
        path.write_bytes(b"!<arch>\n" + header + obj + (b"\n" if len(obj) % 2 else b""))
        row["runtimeArchiveSha256"] = digest(path)
        tool = self.root / "llvm-dis"
        shutil.copy2(pathlib.Path(__file__).parent / "fixtures/native_graph_tools/llvm_dis.py", tool)
        tool.chmod(0o755)
        self.receipt["llvmDisSha256"] = digest(tool)
        return {"EMBEDPDF_LLVM_DIS": str(tool), "FIXTURE_LLVM_TRIPLE": "arm64-apple-ios17.0.0"}

    def test_reviewed_bitcode_reader_checks_the_actual_target_triple(self):
        env = self.bitcode_fixture()
        result = self.run_graph(**env)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_bitcode_wrong_platform_is_rejected(self):
        env = self.bitcode_fixture()
        env["FIXTURE_LLVM_TRIPLE"] = "arm64-apple-macosx14.0.0"
        result = self.run_graph(**env)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("platform", result.stderr)

    def test_bitcode_reader_bytes_must_match_the_frozen_tool_receipt(self):
        env = self.bitcode_fixture()
        self.receipt["llvmDisSha256"] = "f" * 64
        result = self.run_graph(**env)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("llvmDisSha256", result.stderr)

    def test_failed_bitcode_decode_is_not_skipped(self):
        env = self.bitcode_fixture()
        env["FAIL_LLVM_DIS"] = "1"
        result = self.run_graph(**env)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("decode", result.stderr)

    def test_reviewed_bcanalyzer_can_inspect_preserved_bitcode_triples(self):
        import shutil
        env = self.bitcode_fixture()
        tool = pathlib.Path(env.pop("EMBEDPDF_LLVM_DIS"))
        shutil.copy2(pathlib.Path(__file__).parent / "fixtures/native_graph_tools/llvm_bcanalyzer.py", tool)
        tool.chmod(0o755)
        self.receipt["llvmBcanalyzerSha256"] = digest(tool)
        env["EMBEDPDF_LLVM_BCANALYZER"] = str(tool)
        result = self.run_graph(**env)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_unreadable_object_is_not_silently_accepted(self):
        row = self.receipt["targets"]["ios-arm64"]
        path = pathlib.Path(row["runtimeRoot"]) / "lib/libembedpdf.a"
        obj = b"BC\xc0\xdeunknown"
        header = f"bitcode.o/      {0:<12}{0:<6}{0:<6}{'100644':<8}{len(obj):<10}`\n".encode()
        path.write_bytes(b"!<arch>\n" + header + obj + b"\n")
        row["runtimeArchiveSha256"] = digest(path)
        self.rejects("unreadable", "runtime")

class RawBuilderTests(unittest.TestCase):
    def setUp(self):
        import shutil
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = pathlib.Path(self.temp.name)
        source = pathlib.Path(__file__).resolve().parents[1]
        apple = self.root / "platforms/apple"
        apple.mkdir(parents=True)
        shutil.copy2(source / "build-xcframework.sh", apple / "build-xcframework.sh")
        shutil.copy2(source / "check-native-graph.py", apple / "check-native-graph.py")
        (self.root / "Cargo.toml").write_text("[workspace]\n")
        self.tools = self.root / "tools"
        self.tools.mkdir()
        fixtures = pathlib.Path(__file__).parent / "fixtures/native_graph_tools"
        shutil.copy2(fixtures / "bindgen.py", self.tools / "bindgen.py")
        for name in ["cargo", "xcrun", "nm", "libtool", "lipo", "xcodebuild"]:
            path = self.tools / name
            shutil.copy2(fixtures / "tool.py", path)
            path.chmod(0o755)
        script = (apple / "build-xcframework.sh").read_text()
        fixtures = script.split("FIXTURES=(", 1)[1].split(")", 1)[0]
        for fixture in fixtures.splitlines():
            fixture = fixture.strip()
            if not fixture or fixture.startswith("#"):
                continue
            path = self.root / fixture.replace("*", "fixture")
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("fixture")
        for target in TARGETS:
            path = self.root / "runtime" / target / "lib/libembedpdf.a"
            path.parent.mkdir(parents=True)
            path.write_text("archive")

    def run_builder(self, **extra):
        env = dict(os.environ, PATH=str(self.tools) + os.pathsep + os.environ["PATH"],
                   FIXTURE_ROOT=str(self.root), LIB_ROOT=str(self.root / "runtime"), **extra)
        return subprocess.run(["bash", str(self.root / "platforms/apple/build-xcframework.sh")], env=env, capture_output=True, text=True)

    def test_raw_builder_builds_all_four_explicit_fork_and_sdk_pairs(self):
        result = self.run_builder()
        self.assertEqual(result.returncode, 0, result.stderr)
        builds = [json.loads(line) for line in (self.root / "builds.jsonl").read_text().splitlines()]
        self.assertEqual(len(builds), 4)
        for target, (rust, _, sdk, _, _, _) in TARGETS.items():
            row = next(row for row in builds if row["rust"] == rust)
            self.assertEqual(row["fork"], target)
            self.assertEqual(row["sdk"], "/fixture-sdk/" + sdk)
            self.assertIn("--locked", row["args"])
            self.assertIn("--offline", row["args"])

    def test_raw_builder_compares_the_api_from_all_four_built_archives(self):
        result = self.run_builder(SKEW_MAC="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("generated API", result.stderr)
        self.assertEqual(len((self.root / "generated.jsonl").read_text().splitlines()), 4)

    def test_raw_builder_refuses_unreadable_symbol_members(self):
        result = self.run_builder(FAIL_NM="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("symbol", result.stderr)


    def test_raw_builder_refuses_multiple_pdfium_definitions_in_one_archive(self):
        result = self.run_builder(DUPLICATE_PDFIUM="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("duplicate PDFium", result.stderr)

    def test_raw_builder_refuses_duplicate_non_init_pdfium_export(self):
        result = self.run_builder(DUPLICATE_OTHER_PDFIUM="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("duplicate PDFium", result.stderr)

class NativeHashTests(unittest.TestCase):
    def test_native_graph_verifier_and_private_consumer_policy_change_content_identity(self):
        import re
        import shutil
        with tempfile.TemporaryDirectory() as temp:
            root = pathlib.Path(temp)
            source = pathlib.Path(__file__).resolve().parents[1]
            script = source / "apple-engine-hash.sh"
            destination = root / "platforms/apple/apple-engine-hash.sh"
            destination.parent.mkdir(parents=True)
            shutil.copy2(script, destination)
            crates = re.search(r"APPLE_CRATES=\(([^)]*)\)", script.read_text()).group(1).split()
            paths = ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "platforms/apple/build-xcframework.sh",
                     "platforms/apple/package-engine-framework.sh", "platforms/apple/check-native-graph.py",
                     "platforms/apple/macos-consumer.mjs", "perf-compare/scripts/build-arm.mjs"]
            paths += [f"crates/{crate}/src/lib.rs" for crate in crates + ["uniffi-bindgen"]]
            for relative in paths:
                path = root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("fixture source\n")
            subprocess.run(["git", "-C", str(root), "init", "-q"], check=True)
            subprocess.run(["git", "-C", str(root), "add", "."], check=True)
            def identity():
                return subprocess.check_output(["bash", str(destination)], text=True).strip()
            old = identity()
            full = subprocess.check_output(["bash", str(destination), "--full"], text=True).strip()
            self.assertEqual(len(full), 64, "receipt requires the full native source SHA256")
            self.assertEqual(old, full[:16])
            verifier = root / "platforms/apple/check-native-graph.py"
            verifier.write_text("changed native association contract\n")
            new = identity()
            self.assertNotEqual(old, new, "native graph acceptance code must be content-addressed")
            (root / "platforms/apple/macos-consumer.mjs").write_text("changed consumer contract\n")
            self.assertNotEqual(new, identity(), "private consumer validation must be content-addressed")


if __name__ == "__main__":
    unittest.main()
