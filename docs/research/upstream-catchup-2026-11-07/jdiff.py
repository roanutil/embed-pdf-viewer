#!/usr/bin/env python3
"""usage: jdiff.py <rev> <path...>  semantic diff of JSON files vs git rev; prints collapsed paths."""
import json,subprocess,sys,re,collections
rev=sys.argv[1]
SKIP={'covers','description','note','notes'}
def walk(a,b,p,out):
    if isinstance(a,dict) and isinstance(b,dict):
        for k in a.keys()|b.keys():
            if k in SKIP: continue
            q=p+'.'+k
            if k not in a: out['added'][q]+=1
            elif k not in b: out['removed'][q]+=1
            else: walk(a[k],b[k],q,out)
    elif isinstance(a,list) and isinstance(b,list):
        if len(a)!=len(b): out['len'][p+'[]']+=1
        for x,y in zip(a,b): walk(x,y,p+'[]',out)
    elif a!=b:
        key=p
        out['changed'][key]+=1
        ex=out.setdefault('ex',{})
        if key not in ex: ex[key]=(repr(a)[:80],repr(b)[:80])
for f in sys.argv[2:]:
    try: old=json.loads(subprocess.check_output(['git','show',f'{rev}:{f}'],stderr=subprocess.DEVNULL))
    except Exception: print(f"== {f}: new file"); continue
    new=json.load(open(f))
    out={'added':collections.Counter(),'removed':collections.Counter(),'changed':collections.Counter(),'len':collections.Counter()}
    walk(old,new,'',out)
    print(f"== {f}")
    for kind in ('removed','added','len','changed'):
        for k,v in sorted(out[kind].items())[:25]:
            extra=f"  {out['ex'][k][0]} -> {out['ex'][k][1]}" if kind=='changed' else ''
            print(f"  {kind:7} {v:4} {k}{extra}")
