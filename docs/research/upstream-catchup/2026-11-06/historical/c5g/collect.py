import subprocess,json,hashlib,datetime,collections
base='3756c648aa0b9de26744f7939237903bd57a83e8'
head='f28ed4ea4aa46c8e69676558993b103d68b19e08'
commands=[]
def git(*args):
 p=subprocess.run(['git','-C','/workspaces/embed-pdf-viewer',*args],capture_output=True)
 commands.append({'argv':['git','-C','/workspaces/embed-pdf-viewer',*args],'exit':p.returncode,'stdout':p.stdout.decode(),'stderr':p.stderr.decode()})
 return p.stdout.decode()
paths=[]
for line in git('diff','--name-status',base,head).splitlines():
 status,*names=line.split('\t'); paths.append({'status':status,'paths':names})
commits=[]
for line in git('log','--format=%H%x09%s',base+'..'+head).splitlines():
 sha,subject=line.split('\t',1);commits.append({'sha':sha,'subject':subject})
manifest=git('show',head+':platforms/apple/EmbedPDF/Package.swift')
out={'collectedUTC':datetime.datetime.now(datetime.timezone.utc).isoformat(),'repo':'roanutil/embed-pdf-viewer','implKnownLocalSHA':base,'pluginsRemoteSHA':head,'counts':git('rev-list','--left-right','--count',base+'...'+head).strip(),'mergeBase':git('merge-base',base,head).strip(),'missingCommitsByAncestry':commits,'treeDifferences':paths,'manifest':{'blob':git('rev-parse',head+':platforms/apple/EmbedPDF/Package.swift').strip(),'sha256':hashlib.sha256(manifest.encode()).hexdigest(),'contents':manifest},'pinCommit':git('show','--format=fuller','--stat',head),'pinDiff':git('diff',head+'^',head),'gitlinks':git('ls-tree','-r',head).splitlines(),'firstParentCommits':git('log','--first-parent','--format=%H%x09%s',base+'..'+head),'commands':commands}
out['gitlinks']=[x for x in out['gitlinks'] if x.startswith('160000 ')]
out['pathCountsByTopLevel']=dict(collections.Counter(x['paths'][-1].split('/')[0] for x in paths))
print(json.dumps(out,indent=2))
