#!/usr/bin/env python3
"""Synthetic LLVM reader for orchestration tests, never native/ABI evidence."""
import os
import sys
sys.stdin.buffer.read()
if os.environ.get("FAIL_LLVM_DIS") == "1":
    print("fixture bitcode decoding failed", file=sys.stderr)
    sys.exit(1)
print('target triple = "' + os.environ["FIXTURE_LLVM_TRIPLE"] + '"')
