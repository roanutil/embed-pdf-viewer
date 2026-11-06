from pathlib import Path
import json,subprocess,re,hashlib,datetime,ast

root = Path(subprocess.check_output(['git', '-C', str(Path(__file__).resolve().parent), 'rev-parse', '--show-toplevel'], text=True).strip())
out=root/'docs/research/upstream-catchup/2026-11-06'
def run(*args,cwd=root):
 p=subprocess.run(list(args),cwd=cwd,text=True,capture_output=True)
 return {'command':list(args),'exit':p.returncode,'stdout':p.stdout.strip(),'stderr':p.stderr.strip()}
def git(*args,cwd=root): return run('git',*args,cwd=cwd)
p=json.loads((out/'pinned-targets.json').read_text())
checks=[]
for branch,values in p['branches'].items():
 result=git('rev-parse','origin/'+branch)
 assert result['exit']==0 and result['stdout']==values['sha'],result
 checks.append(result)
 result=git('merge-base','--is-ancestor',p['viewer']['sha'],values['sha'])
 assert result['exit']==1,result
 checks.append(result)
result=git('merge-base','--is-ancestor',p['branches']['research/multi-platform-rust-core-impl']['sha'],p['branches']['research/multi-platform-rust-core-impl-plugins']['sha'])
assert result['exit']==0,result
checks.append(result)
result=git('rev-parse','refs/catchup/2026-11-06/viewer-target'); assert result['stdout']==p['viewer']['sha']; checks.append(result)
result=git('ls-tree',p['viewer']['sha'],'packages/engine/runtime/runtime-src'); assert p['runtime']['observedHead'] in result['stdout']; checks.append(result)
result=git('cat-file','-t',p['runtime']['observedHead'],cwd=Path(git('rev-parse','--path-format=absolute','--git-common-dir')['stdout'])/'catchup-runtime.git'); assert result['stdout']=='commit'; checks.append(result)
for file in out.glob('*.json'): json.loads(file.read_text())
for file in out.glob('*.py'): ast.parse(file.read_text())
historical=json.loads((out/'historical-hashes.json').read_text())
for name,sha in historical.items(): assert hashlib.sha256((out/name).read_bytes()).hexdigest()==sha,name

# Create receipt before checking links to that receipt.
(out/'verification.json').write_text('{}\n')
links=0
for file in out.glob('*.md'):
 for link in re.findall(r'\]\(([^)]+)\)',file.read_text()):
  if link.startswith(('https://','http://','mailto:','#')): continue
  relative=link.split('#',1)[0]
  assert (file.parent/relative).exists(),(file.name,relative)
  links+=1
secret_patterns={
 'github-token':r'\b(?:gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{30,})\b',
 'aws-access-key':r'\b(?:AKIA|ASIA)[A-Z0-9]{16}\b',
 'private-key':r'-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----',
 'bearer-credential':r'(?i)authorization["\s:]+bearer\s+[A-Za-z0-9_.-]{20,}',
}
for file in out.rglob('*'):
 if not file.is_file() or file.name=='verify-plan.py': continue
 try: text=file.read_text()
 except UnicodeError: continue
 for category,pattern in secret_patterns.items():
  assert not re.search(pattern,text),f'Possible {category} in {file.relative_to(out)}; inspect before public commit'
status=git('status','--porcelain','--untracked-files=no','--','.',':(exclude)docs/research/upstream-catchup/2026-11-06'); assert status['exit']==0 and not status['stdout'],status
checks.append(status)
all_files=sorted(x for x in out.rglob('*') if x.is_file() and x.name not in ('verification.json','SHA256SUMS'))
manifest=''.join(hashlib.sha256(x.read_bytes()).hexdigest()+'  '+str(x.relative_to(out))+'\n' for x in all_files)
(out/'SHA256SUMS').write_text(manifest)
result=run('sha256sum','--check','SHA256SUMS',cwd=out); assert result['exit']==0,result
result['stdout']=str(len(all_files))+' files verified'
checks.append(result)
receipt={'verifiedUTC':datetime.datetime.now(datetime.timezone.utc).isoformat(),'scope':'Document, pin, source-cleanliness and hash checks only; product evidence is in execution-state.json and execution-evidence/','canonicalViewerTarget':p['viewer']['sha'],'viewerSelectedRuntime':p['runtime']['observedHead'],'publicBranchTips':{k:v['sha'] for k,v in p['branches'].items()},'currentMarkdownLinksChecked':links,'historicalFilesByteExact':len(historical),'hashedFiles':len(all_files),'secretPatternScan':'No matches for GitHub tokens, AWS access keys, private keys or bearer credentials','checks':checks,'limitations':['C7 historical bare metadata excluded; original host records retained','Full catch-up and final packaged-consumer acceptance are not established by this document checker','No dotnet installed in current ARM64 image; Android NDK and Apple hosts unavailable here','No product integration/behavior/platform acceptance claimed']}
(out/'verification.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps({k:v for k,v in receipt.items() if k!='checks'},indent=2))
