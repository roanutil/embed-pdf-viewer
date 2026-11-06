#!/usr/bin/env python3
"""Check the reviewed Swift API inventory against Rust and Swift source changes."""
import argparse
import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[2]
PURE = {'acrojs', 'annotation-core', 'fonts', 'form-core', 'geometry', 'signature-core', 'stage-core', 'ui-core'}


def inventory(root):
    result = []
    for directory in sorted((root / 'crates/plugins').glob('*')):
        if directory.name in PURE:
            continue
        for path in sorted((directory / 'src').rglob('*.rs')):
            if any(part in {'ffi', 'harness', 'tests'} for part in path.parts) or path.stem in {'ffi', 'harness'}:
                continue
            source = path.read_text()
            # Capability impls, including Core reached through Deref, and their
            # nested read/write modules. Free helpers aren't capability members.
            for impl in re.finditer(r'^impl(?:<[^\n]*>)?\s+([\w:]+)(?:<[^\n{]*>)?\s*\{', source, re.M):
                owner = impl.group(1).split('::')[-1]
                if not (owner.endswith('Capability') or owner in {'Core', 'ViewDemand'}):
                    continue
                depth, end = 1, impl.end()
                while end < len(source) and depth:
                    depth += (source[end] == '{') - (source[end] == '}')
                    end += 1
                for method in re.finditer(r'\bpub\s+(?:async\s+)?fn\s+(\w+)\s*[(<]', source[impl.end():end]):
                    result.append({'crate': directory.name, 'path': str(path.relative_to(root)),
                                   'owner': owner, 'symbol': method.group(1)})
    return result


def key(member):
    return member['path'], member['owner'], member['symbol']


def validate(root, manifest, discovered, allow_pending=False):
    errors = []
    entries = {key(member): member for member in manifest['members']}
    if len(entries) != len(manifest['members']):
        errors.append('duplicate manifest member')
    actual = {key(member) for member in discovered}
    for member in discovered:
        if key(member) not in entries:
            errors.append(f"unaccounted Rust member: {member['path']}::{member['symbol']}")
    for identity, member in entries.items():
        if identity not in actual:
            errors.append(f'stale Rust member: {identity}')
        if member.get('exclusion'):
            continue
        if member.get('pending') and allow_pending:
            continue
        if not member.get('export') or not member.get('swift'):
            errors.append(f'incomplete mapping: {identity}')
            continue
        for language, field, pattern in [
            ('Rust export', 'export', r'\bpub\s+(?:async\s+)?fn\s+{symbol}\s*[(<]'),
            ('Swift', 'swift', r'\bpublic\s+(?:var|let|func)\s+`?{symbol}`?\b'),
        ]:
            target = member[field]
            path = root / target['path']
            if not path.exists() or not re.search(pattern.format(symbol=re.escape(target['symbol'])), path.read_text()):
                errors.append(f"missing {language}: {target}")
    actual_crates = {path.parent.name for path in (root / 'crates/plugins').glob('*/Cargo.toml')}
    if actual_crates and set(manifest['crates']) != actual_crates:
        errors.append('crate coverage does not match crates/plugins inventory')
    return errors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--allow-pending', action='store_true', help='during implementation only')
    args = parser.parse_args()
    manifest = json.loads((ROOT / 'platforms/apple/swift-api-coverage.json').read_text())
    errors = validate(ROOT, manifest, inventory(ROOT), args.allow_pending)
    for error in errors:
        print(error, file=sys.stderr)
    if errors:
        return 1
    print(f"Swift coverage: {len(manifest['crates'])} crates, {len(manifest['members'])} members accounted for")
    return 0


if __name__ == '__main__':
    sys.exit(main())
