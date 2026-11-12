# gaps.py: uncovered statement lines (vectors/coverage.json) that upstream added in 074a190c9..ed064e0c8,
# grouped by package, with the upstream commit that last touched each line (git blame at ed064e0c8).
import json, re, subprocess, collections
root = '/workspaces/wt/plugins'
cov = json.load(open(f'{root}/tooling/plugin-conformance/vectors/coverage.json'))
def ranges(v):
    out = set()
    for x in v:
        if isinstance(x, int): out.add(x)
        else:
            m = re.match(r'(\d+)-(\d+)', str(x))
            if m: out.update(range(int(m.group(1)), int(m.group(2)) + 1))
            elif str(x).isdigit(): out.add(int(x))
    return out
diff = subprocess.check_output(['git', '-C', root, 'diff', '-U0', '074a190c9', 'ed064e0c8', '--', 'packages']).decode(errors='replace')
added = collections.defaultdict(set); cur = None
for line in diff.splitlines():
    if line.startswith('+++ b/'): cur = line[6:]
    elif line.startswith('@@') and cur:
        m = re.search(r'\+(\d+)(?:,(\d+))?', line); a = int(m.group(1)); n = int(m.group(2) or 1)
        added[cur].update(range(a, a + n))
res = collections.defaultdict(list)
for path, info in cov['files'].items():
    if path not in added: continue
    hit = sorted(ranges(info.get('uncoveredLines', [])) & added[path])
    if not hit: continue
    blame = subprocess.check_output(['git', '-C', root, 'blame', '-s', '-l', 'ed064e0c8', '--', path]).decode(errors='replace').splitlines()
    commits = sorted({blame[l - 1][:9] for l in hit if l - 1 < len(blame)})
    pkg = '/'.join(path.split('/')[1:3])
    spans = []; 
    for l in hit:
        if spans and l == spans[-1][1] + 1: spans[-1][1] = l
        else: spans.append([l, l])
    res[pkg].append((path, ','.join(f'{a}' if a == b else f'{a}-{b}' for a, b in spans), len(hit), commits))
new_unloaded = [u for u in cov['unloaded'] if u in added]
for pkg in sorted(res):
    print(f'## {pkg}')
    for path, sp, n, commits in res[pkg]: print(f'- {path}: lines {sp} ({n}; {" ".join(commits)})')
print('## unloaded files upstream touched'); [print('-', u) for u in new_unloaded]
