# chainpins.py: for each changed file under historical/ (vs HEAD), replace pins of its old sha256 with
# its new sha256 across historical/ and test/, repeating until no pinned file changes. Prints each swap.
import hashlib, os, subprocess
root = '/workspaces/wt/plugins'; base = 'tooling/plugin-conformance'
def changed():
    out = subprocess.check_output(['git', '-C', root, 'diff', '--name-only', 'HEAD', '--', f'{base}/historical', f'{base}/test', f'{base}/src']).decode().split()
    return out
def sha_head(f):
    return hashlib.sha256(subprocess.check_output(['git', '-C', root, 'show', f'HEAD:{f}'])).hexdigest()
def sha_now(f):
    return hashlib.sha256(open(f'{root}/{f}', 'rb').read()).hexdigest()
targets = []
for d in ('historical', 'test'):
    for dp, _, fs in os.walk(f'{root}/{base}/{d}'):
        targets += [os.path.join(dp, f) for f in fs]
done = {}
for _ in range(20):
    swaps = {}
    for f in changed():
        old, new = sha_head(f), sha_now(f)
        old = done.get(f, old)
        if old != new: swaps[f] = (old, new)
    if not swaps: break
    for f, (old, new) in swaps.items():
        done[f] = new
        for t in targets:
            try:
                text = open(t).read()
            except UnicodeDecodeError:
                continue
            if old in text:
                open(t, 'w').write(text.replace(old, new))
                print(f'{os.path.relpath(t, root)}: pin of {f} {old[:12]} -> {new[:12]}')
