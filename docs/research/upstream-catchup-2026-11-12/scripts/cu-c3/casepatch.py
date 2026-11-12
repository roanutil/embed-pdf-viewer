# casepatch.py <rev> <file> <case> [maxlines]: unified diff of one case, old vs worktree
import json, subprocess, sys, difflib
rev, path, name = sys.argv[1:4]; mx = int(sys.argv[4]) if len(sys.argv) > 4 else 120
old = json.loads(subprocess.check_output(['git', 'show', f'{rev}:{path}']))
new = json.load(open(path))
pick = lambda d: next(c for c in d['cases'] if c['name'] == name)
a = json.dumps(pick(old), indent=1, ensure_ascii=False).splitlines()
b = json.dumps(pick(new), indent=1, ensure_ascii=False).splitlines()
out = list(difflib.unified_diff(a, b, lineterm='', n=4))
print('\n'.join(l[:240] for l in out[:mx]))
