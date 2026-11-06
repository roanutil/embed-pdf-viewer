import subprocess,json,hashlib,pathlib,datetime
out=pathlib.Path('/private/tmp/kkt-peach-lemur/c5g')
commands=[['docker','inspect','embedpdf-c5-combined','--format','{{.Image}}'],['docker','exec','embedpdf-c5-combined','git','-C','/workspaces/embed-pdf-viewer','remote','-v'],['docker','exec','embedpdf-c5-combined','git','-C','/workspaces/embed-pdf-viewer','ls-remote','fork','refs/heads/research/catchup-2026-11-combined','refs/heads/research/multi-platform-rust-core-impl','refs/heads/research/multi-platform-rust-core-impl-plugins']]
for repo,branch in [('embed-pdf-viewer-private','research/multi-platform-rust-core-impl'),('embed-pdf-viewer-private','research/multi-platform-rust-core-impl-plugins')]:commands.append(['/opt/homebrew/bin/gh','api','repos/roanutil/'+repo+'/git/ref/heads/'+branch])
for tag in ['apple-engine-3816be131b65bdee','runtime-payloads-e352d80f0a7c2851']:commands.append(['/opt/homebrew/bin/gh','api','repos/roanutil/embed-pdf-runtime/git/ref/tags/'+tag])
results=[]
for n,cmd in enumerate(commands):
 p=subprocess.run(cmd,capture_output=True);log=out/f'metadata-{n:02}.log';log.write_bytes(p.stdout+p.stderr)
 results.append({'argv':cmd,'exit':p.returncode,'log':str(log),'sha256':hashlib.sha256(log.read_bytes()).hexdigest()})
 print(n,p.returncode,p.stdout.decode()[:1000],p.stderr.decode()[:300])
(out/'fresh-metadata.json').write_text(json.dumps({'utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'commands':results},indent=2)+'\n')
