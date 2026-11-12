# srcpins.py [--write]: find {path, sha256, commit} rows under historical/ whose package source changed;
# with --write, re-pin them to the current bytes and the last commit that touched the path at HEAD.
import hashlib, json, os, subprocess, sys
root = '/workspaces/wt/plugins'; hist = f'{root}/tooling/plugin-conformance/historical'
write = '--write' in sys.argv
cache = {}; prevc = {}
def prev(path):
    if path not in prevc:
        prevc[path] = subprocess.check_output(['git', '-C', root, 'log', '-1', '--format=%H', '7990560d6634298ae6767d9e30626102e124e981', '--', path]).decode().strip()
    return prevc[path]
def last(path):
    if path not in cache:
        c = subprocess.check_output(['git', '-C', root, 'log', '-1', '--format=%H', 'HEAD', '--', path]).decode().strip()
        b = subprocess.check_output(['git', '-C', root, 'show', f'{c}:{path}'])
        cache[path] = (c, hashlib.sha256(b).hexdigest())
    return cache[path]
def walk(node, hits, file):
    if isinstance(node, dict):
        if set(('path', 'sha256', 'commit')) <= node.keys() and isinstance(node['path'], str) and node['path'].startswith('packages/'):
            p = f"{root}/{node['path']}"
            if os.path.exists(p):
                cur = hashlib.sha256(open(p, 'rb').read()).hexdigest()
                if cur != node['sha256'] and prev(node['path']) == node['commit']:
                    c, h = last(node['path'])
                    assert h == cur, (node['path'], 'HEAD last commit bytes differ from worktree')
                    hits.append((file, node['path'], node['sha256'][:12], cur[:12], node['commit'][:9], c[:9]))
                    if write: node['sha256'], node['commit'] = cur, c
        for v in node.values(): walk(v, hits, file)
    elif isinstance(node, list):
        for v in node: walk(v, hits, file)
import re
for d, _, fs in os.walk(hist):
    for f in sorted(fs):
        if not f.endswith('.json'): continue
        path = os.path.join(d, f); text = open(path).read(); data = json.loads(text); hits = []
        walk(data, hits, os.path.relpath(path, hist))
        for h in hits: print(*h)
        if write and hits:
            for _f, rel, _o, _n, _oc, _nc in hits:
                c, cur = last(rel); old = None
            # in-place text edit: each row keeps its formatting, only sha256/commit values change
            def fix(m):
                row = m.group(0)
                pm = re.search(r'"path":\s*"([^"]+)"', row)
                if not pm or pm.group(1) not in cache: return row
                rp = pm.group(1)
                if prev(rp) not in row: return row
                p2 = f"{root}/{rp}"
                cur = hashlib.sha256(open(p2, 'rb').read()).hexdigest()
                sm = re.search(r'"sha256":\s*"([0-9a-f]{64})"', row)
                if not sm or sm.group(1) == cur: return row
                c, _ = last(rp)
                return row.replace(sm.group(1), cur).replace(prev(rp), c)
            text = re.sub(r'\{[^{}]*"path":\s*"packages/[^"]+"[^{}]*\}', fix, text)
            open(path, 'w').write(text)
