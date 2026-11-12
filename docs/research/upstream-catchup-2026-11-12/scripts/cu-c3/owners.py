# owners.py [--write]: currentOwners rows in historical/*/source-evidence.json whose package source changed:
# re-pin sha256 (and definition, lastOwnerCommit where the row has them) to the file at HEAD.
import hashlib, json, os, subprocess, sys, glob
root = '/workspaces/wt/plugins'; hist = f'{root}/tooling/plugin-conformance/historical'
write = '--write' in sys.argv
only = [a for a in sys.argv[1:] if not a.startswith('--')]
for path in sorted(glob.glob(f'{hist}/*/source-evidence.json')):
    if only and os.path.basename(os.path.dirname(path)) not in only: continue
    text = open(path).read(); data = json.loads(text)
    if not isinstance(data, dict) or 'currentOwners' not in data: continue
    hits = 0
    for row in data['currentOwners']:
        p = f"{root}/{row['path']}"
        if not os.path.exists(p): print('MISSING', row['path']); continue
        b = open(p, 'rb').read(); cur = hashlib.sha256(b).hexdigest()
        if cur == row['sha256']: continue
        c = subprocess.check_output(['git', '-C', root, 'log', '-1', '--format=%H', 'HEAD', '--', row['path']]).decode().strip()
        print(os.path.relpath(path, hist), row['path'], row['sha256'][:12], '->', cur[:12], 'owner', c[:9])
        hits += 1
        if write:
            row['sha256'] = cur
            if 'definition' in row: row['definition'] = b.decode()
            if 'lastOwnerCommit' in row: row['lastOwnerCommit'] = c
    if write and hits:
        for indent in (2, None):
            if json.dumps(json.loads(text), indent=indent, ensure_ascii=False) + '\n' == text: break
        else:
            sys.exit(f'{path}: unknown formatting, not rewritten')
        open(path, 'w').write(json.dumps(data, indent=indent, ensure_ascii=False) + '\n')
