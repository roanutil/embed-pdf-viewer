import subprocess,pathlib,json,datetime
records=[]
def run(argv):
 p=subprocess.run(argv,capture_output=True,text=True);records.append({'argv':argv,'exit':p.returncode,'stdout':p.stdout,'stderr':p.stderr})
 if p.returncode:print(json.dumps({'state':'FAIL','commands':records},indent=2));raise SystemExit(1)
 return p.stdout.strip()
repos=[('viewer','/workspaces/embed-pdf-viewer','https://github.com/embedpdf/embed-pdf-viewer.git','4ad8986099314b9587df423c802db103a21d665d','f28ed4ea4aa46c8e69676558993b103d68b19e08'),('runtime','/Users/andrew/repos/embed-pdf-runtime','https://github.com/embedpdf/runtime.git','eeac787c02fac7b40c74c91bbff0ffb5f59ff97e','3c55b7a1c0bf956231392a573e87f28dd6d6667d')]
for name,source,url,target,base in repos:
 d=pathlib.Path('/tmp/embedpdf-c7-'+name+'-metadata.git')
 if d.exists():raise SystemExit('STOP: own metadata directory already exists')
 common=pathlib.Path(run(['git','-C',source,'rev-parse','--git-common-dir']))
 if not common.is_absolute():common=(pathlib.Path(source)/common).resolve()
 run(['git','init','--bare',str(d)]);(d/'objects/info/alternates').write_text(str(common/'objects')+'\n')
 run(['git','--git-dir',str(d),'update-ref','refs/c7/reviewed',base])
 run(['git','--git-dir',str(d),'fetch','--no-tags',url,target+':refs/c7/upstream'])
 run(['git','--git-dir',str(d),'show','-s','--format=%H%n%cI%n%aI%n%s',target])
 if name=='viewer':run(['git','--git-dir',str(d),'ls-tree',target,'packages/engine/runtime/runtime-src']);run(['git','--git-dir',str(d),'show',target+':.gitmodules'])
print(json.dumps({'state':'PASS','utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'commands':records},indent=2))
