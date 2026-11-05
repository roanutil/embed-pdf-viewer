#!/usr/bin/env python3
"""Synthetic bitcode metadata tool; not native/ABI evidence."""
import os
import sys
sys.stdin.buffer.read()
triple = os.environ["FIXTURE_LLVM_TRIPLE"]
print('<TRIPLE ' + ' '.join(f'op{i}={ord(c)}' for i, c in enumerate(triple)) + '/>')
