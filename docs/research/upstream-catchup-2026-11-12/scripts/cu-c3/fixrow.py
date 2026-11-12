# fixrow.py: re-pin the fixed-bytes-stamp-tool accounting row to its current input and covers (text edit).
import json, re
p = '/workspaces/wt/plugins/tooling/plugin-conformance/historical/task-12b2b1b-ii/accounting.json'
s = open(p).read()
cov = json.load(open('/workspaces/wt/plugins/tooling/plugin-conformance/vectors/plugins/annotation-tools.json'))
case = next(c for c in cov['cases'] if c['name'] == 'fixed-bytes-stamp-tool')
start = s.index('"before": "fixed-bytes-stamp-tool"'); end = s.index('}', start)
row = s[start:end]
new = row.replace('0a12ff056e2de681627fcefbbfbd3110774da04137cbcb8d3a596be31876478e',
                  '7f5ae7b93b8fcab442a7adbf16ef3336b2cfd1a88e27efe7d1136bc208891250')
new = re.sub(r'"afterCovers": "(?:[^"\\]|\\.)*"', lambda m: '"afterCovers": ' + json.dumps(case['covers'], ensure_ascii=False), new)
assert new != row
open(p, 'w').write(s[:start] + new + s[end:])
print('ok')
