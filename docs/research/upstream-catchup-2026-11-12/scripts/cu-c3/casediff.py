# casediff.py <rev> <file>: list case names whose content changed between rev and worktree, with covers
import json, subprocess, sys
rev, path = sys.argv[1], sys.argv[2]
old = json.loads(subprocess.check_output(['git', 'show', f'{rev}:{path}']))
new = json.load(open(path))
o = {c['name']: c for c in old['cases']}; n = {c['name']: c for c in new['cases']}
for k in o:
    if k not in n: print('REMOVED', k)
for k in n:
    if k not in o: print('ADDED', k)
    elif json.dumps(o[k]) != json.dumps(n[k]): print('CHANGED', k, '::', (n[k].get('covers') or '')[:200])
