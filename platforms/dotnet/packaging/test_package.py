import json
from pathlib import Path
import tempfile
import unittest
import copy
import os
import platform
import subprocess
import sys
import hashlib
import zipfile

from schema import validate

class SchemaTests(unittest.TestCase):
    def test_record_rejects_dirty_source_without_patch_hash(self):
        root = Path(__file__).resolve().parents[1]
        schema = json.loads((root / "validation/record.schema.json").read_text())
        record = {"schemaVersion":1,"slice":6,"rid":"linux-arm64",
            "source":{"commit":"a"*40,"dirty":True,"runtimeFork":"b"*40},
            "environment":{"kind":"container","os":"Linux","arch":"arm64","emulated":False,
                "tools":{},"imageId":"sha256:"+"c"*64},
            "inputs":[],"commands":[{"command":"test","cwd":".","exitCode":0,"results":"pass"}],
            "artifacts":[],"limitations":[]}
        with self.assertRaisesRegex(ValueError, "diffSha256"):
            validate(record, schema)
        record["source"]["diffSha256"] = "d"*64
        validate(record, schema)

    def test_record_rejects_container_without_digest_and_unknown_fields(self):
        with self.assertRaises(ValueError):
            validate({"kind":"container"}, {"type":"object","additionalProperties":False,"properties":{}})
        with self.assertRaises(ValueError):
            validate(True, {"type":"integer"})

class ManifestTests(unittest.TestCase):
    def test_emulated_status_is_distinct_and_only_linux_x64(self):
        root = Path(__file__).resolve().parents[1]
        schema = json.loads((root/"package-manifest.schema.json").read_text())
        states = schema["properties"]["rids"]["properties"]
        validate({"status":"container-emulated","reason":"No native x64 run"}, states["linux-x64"])
        for rid in ["linux-arm64","osx-arm64","osx-x64","win-x64"]:
            with self.assertRaises(ValueError):
                validate({"status":"container-emulated","reason":"No native run"}, states[rid])

class SingleRidTests(unittest.TestCase):
    def setUp(self):
        from package import RIDS
        self.root = Path(__file__).resolve().parents[1]
        self.schema = json.loads((self.root / "package-manifest.schema.json").read_text())
        file = {"path":"package.nupkg","sha256":"a"*64,"bytes":1}
        self.manifest = {"schemaVersion":1,"packageId":"EmbedPDF.Engine","version":"0.0.0-local",
            "source":{"commit":"a"*40,"dirty":False},"runtimeFork":"b"*40,
            "runtimeManifest":file,"fontManifest":file,"abi":1,
            "rids":{rid:{"status":"PACKED_UNVALIDATED" if rid == "linux-arm64" else "UNVALIDATED",
                         "reason":"test evidence"} for rid in RIDS},"files":[file],"limitations":[]}

    def test_schema_rejects_success_claims_for_other_rids(self):
        for rid in ["osx-x64", "win-x64", "linux-x64"]:
            for status in ["VALIDATED", "PACKED_UNVALIDATED"]:
                forged = copy.deepcopy(self.manifest)
                forged["rids"][rid]["status"] = status
                with self.subTest(rid=rid, status=status), self.assertRaises(ValueError):
                    validate(forged, self.schema)
        forged = copy.deepcopy(self.manifest)
        forged["rids"]["linux-x64"]["status"] = "container-emulated"
        with self.assertRaises(ValueError):
            validate(forged, self.schema)
        validate(self.manifest, self.schema)

    def test_schema_rejects_empty_file_evidence(self):
        self.manifest["files"] = []
        with self.assertRaises(ValueError):
            validate(self.manifest, self.schema)

    def run_tool(self, manifest, identity, action="smoke", empty_files=False):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            package = directory / "package.nupkg"
            with zipfile.ZipFile(package, "w") as bundle:
                bundle.writestr("runtimes/linux-arm64/native/epdf-assets.json", json.dumps(identity))
            manifest = copy.deepcopy(manifest)
            manifest["files"] = [{"path":package.name,"sha256":hashlib.sha256(package.read_bytes()).hexdigest(),"bytes":package.stat().st_size}]
            if empty_files:
                manifest["files"] = []
            path = directory / "manifest.json"
            path.write_text(json.dumps(manifest))
            command = [sys.executable, str(self.root / "packaging/package.py"), action,
                "--rid", "linux-arm64", "--package", str(package), "--manifest", str(path)]
            if action == "validate":
                command += ["--record", str(directory / "record.json")]
            env = dict(os.environ, EPDF_IMAGE_ID="sha256:"+"c"*64)
            return subprocess.run(command, env=env, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)

    def identity(self):
        return {key:self.manifest[key] for key in ["source","runtimeFork","abi"]} | {"rid":"linux-arm64","files":[]}

    @unittest.skipUnless(platform.system() == "Linux" and platform.machine() == "aarch64", "CLI regression requires native Linux ARM64 author container")
    def test_actual_tool_rejects_forged_claims_and_empty_assets(self):
        forged = copy.deepcopy(self.manifest)
        forged["rids"]["osx-x64"]["status"] = "VALIDATED"
        for action in ["smoke", "validate"]:
            result = self.run_tool(forged, self.identity(), action)
            self.assertNotEqual(0, result.returncode)
            self.assertIn("rids.osx-x64.status", result.stdout)
        result = self.run_tool(self.manifest, self.identity())
        self.assertNotEqual(0, result.returncode)
        self.assertIn("native asset inventory", result.stdout)
        result = self.run_tool(self.manifest, self.identity(), empty_files=True)
        self.assertNotEqual(0, result.returncode)
        self.assertIn("files: too few items", result.stdout)

    @unittest.skipUnless(platform.system() == "Linux" and platform.machine() == "aarch64", "CLI regression requires native Linux ARM64 author container")
    def test_actual_tool_rejects_mismatched_rid(self):
        identity = self.identity() | {"rid":"win-x64"}
        result = self.run_tool(self.manifest, identity)
        self.assertNotEqual(0, result.returncode)
        self.assertIn("packaged RID identity", result.stdout)

class NativeInspectionTests(unittest.TestCase):
    def test_rejects_wrong_or_incomplete_abi_exports(self):
        from package import check_exports
        with self.assertRaisesRegex(ValueError, "epdf_abi_version"):
            check_exports("FPDF_InitLibrary")
        with self.assertRaisesRegex(ValueError, "epdf_engine_create"):
            check_exports("000000 T _epdf_abi_version", "_")

    def test_rejects_build_machine_rpath(self):
        from package import check_elf
        with self.assertRaisesRegex(ValueError, "relative"):
            check_elf("libepdf_capi.so", "Dynamic section: (RUNPATH) Library runpath: [/home/build/pdfium]", "")
        check_elf("libepdf_capi.so", "Dynamic section: (RUNPATH) Library runpath: [$ORIGIN]", "")

    def test_rejects_missing_dependency_and_absolute_dependency(self):
        from package import check_elf
        with self.assertRaisesRegex(ValueError, "missing"):
            check_elf("libepdf_capi.so", "(RUNPATH) Library runpath: [$ORIGIN]", "libembedpdf.so => not found")
        with self.assertRaisesRegex(ValueError, "absolute"):
            check_elf("libepdf_capi.so", "(NEEDED) Shared library: [/build/libembedpdf.so]\n(RUNPATH) Library runpath: [$ORIGIN]", "")

if __name__ == "__main__":
    unittest.main()
