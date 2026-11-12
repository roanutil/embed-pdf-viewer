# trace.py <file> <case> [width]: one line per step: step, outcome, engine paths, settled, reports
import json, sys
d = json.load(open(sys.argv[1])); w = int(sys.argv[3]) if len(sys.argv) > 3 else 260
c = next(c for c in d['cases'] if c['name'] == sys.argv[2])
for i, (s, t) in enumerate(zip(c['steps'], c['trace'])):
    st = s.get('method') or s.get('op'); args = json.dumps(s.get('args', s.get('label', s.get('event', {}).get('type') if isinstance(s.get('event'), dict) else '')))[:70]
    print(f"{i:2} {st} {args} => {json.dumps(t.get('outcome'))[:w]}")
    if t.get('engine'): print('     engine:', [e.get('path', e) for e in t['engine']])
    if t.get('settled'): print('     settled:', json.dumps(t['settled'])[:w])
    if t.get('reports'): print('     reports:', json.dumps(t['reports'])[:w])
