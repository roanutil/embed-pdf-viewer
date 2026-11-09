#!/usr/bin/env python3
"""Check the reviewed Swift API inventory against Rust and Swift source changes."""
import argparse
import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[2]
PURE = {'acrojs', 'annotation-core', 'fonts', 'form-core', 'geometry', 'signature-core', 'stage-core', 'text-core', 'ui-core'}


def inventory(root):
    result = []
    directories = [(directory, directory.name) for directory in sorted((root / 'crates/plugins').glob('*'))
                   if directory.name not in PURE]
    directories.append((root / 'crates/epdf-kernel', 'epdf-kernel'))
    for directory, crate in directories:
        for path in sorted((directory / 'src').rglob('*.rs')):
            if any(part in {'ffi', 'harness', 'tests'} for part in path.relative_to(directory).parts) or path.stem in {'ffi', 'harness'}:
                continue
            source = path.read_text()
            # Capability impls, including nested read/write modules. Kernel
            # Documents and Kernel are included; other kernel types are resources.
            for impl in re.finditer(r'^impl(?:<[^\n]*>)?\s+([\w:]+)(?:<[^\n{]*>)?\s*\{', source, re.M):
                owner = impl.group(1).split('::')[-1]
                if crate == 'epdf-kernel':
                    if owner not in {'Documents', 'Kernel'}:
                        continue
                elif not (owner.endswith('Capability') or owner in {'Core', 'ViewDemand'}):
                    continue
                depth, end = 1, impl.end()
                while end < len(source) and depth:
                    depth += (source[end] == '{') - (source[end] == '}')
                    end += 1
                for method in re.finditer(r'\bpub\s+(?:async\s+)?fn\s+(\w+)\s*[(<]', source[impl.end():end]):
                    result.append({'crate': crate, 'path': str(path.relative_to(root)),
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
        mappings = [member] + member.get('overloads', [])
        for mapping in mappings:
            for language, field, pattern in [
                ('Rust export', 'export', r'\bpub\s+(?:async\s+)?fn\s+{symbol}\s*[(<]'),
                ('Swift', 'swift', r'\bpublic\s+(?:var|let|func)\s+`?{symbol}`?\b'),
            ]:
                target = mapping.get(field)
                if not target:
                    errors.append(f'incomplete overload mapping: {identity}')
                    continue
                path = root / target['path']
                if not path.exists() or not re.search(pattern.format(symbol=re.escape(target['symbol'])), path.read_text()):
                    errors.append(f"missing {language}: {target}")
    actual_crates = {path.parent.name for path in (root / 'crates/plugins').glob('*/Cargo.toml')}
    if actual_crates and set(manifest['crates']) != actual_crates:
        errors.append('crate coverage does not match crates/plugins inventory')
    kernel = manifest.get('kernel', {})
    if (root / 'crates/epdf-kernel/src').exists() and kernel.get('kind') != 'core':
        errors.append('kernel coverage must explicitly classify core and internal resources')
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
    mapped = sum(not member.get('exclusion') for member in manifest['members'])
    excluded = len(manifest['members']) - mapped
    print(f"Swift API accounting: {len(manifest['crates'])} plugin-directory crates + kernel, "
          f"{len(manifest['members'])} members ({mapped} mapped, {excluded} exclusions). "
          "Counts describe accounting, not implementation parity.")
    return 0


if __name__ == '__main__':
    sys.exit(main())
