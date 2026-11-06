import subprocess,json,datetime
out={'utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'commands':[]}
def g(d,*args):
 p=subprocess.run(['git','--git-dir',d,*args],capture_output=True,text=True);out['commands'].append({'argv':p.args,'exit':p.returncode,'stdout':p.stdout,'stderr':p.stderr});return p.stdout.strip()
v='/tmp/embedpdf-c7-viewer-metadata.git';r='/tmp/embedpdf-c7-runtime-metadata.git';old='f28ed4ea4aa46c8e69676558993b103d68b19e08';new='4ad8986099314b9587df423c802db103a21d665d';rold='3c55b7a1c0bf956231392a573e87f28dd6d6667d';rnew='f1ba46f0dc3dc5f9b055a7311dda0c13ecc0f185';rhead='eeac787c02fac7b40c74c91bbff0ffb5f59ff97e'
for d,a,b in [(v,old,new),(r,rold,rnew),(r,rnew,rhead)]:
 base=g(d,'merge-base',a,b);g(d,'rev-list','--left-right','--count',a+'...'+b);g(d,'log','--no-merges','--format=%H%x09%cI%x09%s',base+'..'+b);g(d,'diff','--name-status',base,b);g(d,'diff','--stat',base,b)
for sha in [rnew,rhead]:g(r,'show','-s','--format=%H%n%cI%n%s',sha)
for path in ['packages/engine/core/src/wire/worker-protocol.ts','packages/engine/runtime/engine-runtime-build.local.json','packages/engine/runtime/src/core/pdf-functions.generated.ts']:
 g(v,'diff',old,new,'--',path)
print(json.dumps(out,indent=2))
