import sys,json,os,subprocess,datetime
q=json.load(sys.stdin);token=q['credential'];env={**os.environ,'GH_TOKEN':token,'GH_HOST':'github.com'};records=[]
for repo in ['embedpdf/embed-pdf-viewer','embedpdf/runtime']:
 cmd=['gh','api','repos/'+repo,'--jq','{full_name,html_url,default_branch,fork}'];p=subprocess.run(cmd,capture_output=True,text=True,env=env);records.append({'argv':cmd,'exit':p.returncode,'stdout':p.stdout.replace(token,'[REDACTED]'),'stderr':p.stderr.replace(token,'[REDACTED]')})
for branch in ['research/multi-platform-rust-core-impl','research/multi-platform-rust-core-impl-plugins']:
 cmd=['gh','api','repos/roanutil/embed-pdf-viewer-private/git/ref/heads/'+branch];p=subprocess.run(cmd,capture_output=True,text=True,env=env);records.append({'argv':cmd,'exit':p.returncode,'stdout':p.stdout.replace(token,'[REDACTED]'),'stderr':p.stderr.replace(token,'[REDACTED]')})
print(json.dumps({'utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'commands':records},indent=2))
