import subprocess,json
out=[]
def g(*args):
 p=subprocess.run(['git','--git-dir','/tmp/embedpdf-c7-viewer-metadata.git',*args],capture_output=True,text=True);out.append({'argv':p.args,'exit':p.returncode,'stdout':p.stdout,'stderr':p.stderr});return p.stdout
old='f28ed4ea4aa46c8e69676558993b103d68b19e08'
g('ls-tree','-r','--name-only',old,'crates/epdf-engine/tests','crates/pdfium/tests')
for path in ['crates/epdf-engine/src/features/annotation/update.rs','crates/epdf-engine/src/features/annotation/fields.rs','crates/epdf-engine/src/features/annotation/write/style.rs','crates/epdf-engine/src/features/fonts/document.rs','packages/core/annotation/src/ink-curves.ts']:
 g('show',(old if path.startswith('crates') else '4ad8986099314b9587df423c802db103a21d665d')+':'+path)
g('grep','-n','-E','dash_array|dashArray|subtype cannot|cannot change|same subtype',old,'--','crates/epdf-engine/src/features/annotation')
print(json.dumps(out,indent=2))
