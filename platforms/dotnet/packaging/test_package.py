import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

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

class NativeInspectionTests(unittest.TestCase):
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
