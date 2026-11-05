#!/usr/bin/env python3
import os, pathlib, sys
args = sys.argv[1:]
out = pathlib.Path(args[args.index("--out-dir") + 1]); out.mkdir(parents=True, exist_ok=True)
lib = args[args.index("--library") + 1]
with pathlib.Path(os.environ["FIXTURE_ROOT"], "generated.jsonl").open("a") as log:
    log.write(lib + "\n")
skew = os.environ.get("SKEW_MAC") == "1" and "x86_64-apple-darwin" in lib
(out / "epdf.swift").write_text("if uniffi_epdf_checksum_test() != " + ("124" if skew else "123") + " {}\n")
(out / "epdfFFI.h").write_text("unsigned short uniffi_epdf_checksum_test(void);\n")
(out / "epdfFFI.modulemap").write_text('module epdfFFI { header "epdfFFI.h" export * }\n')
