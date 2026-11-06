import subprocess,json
r='/workspaces/embed-pdf-viewer';i='3756c648aa0b9de26744f7939237903bd57a83e8';p='f28ed4ea4aa46c8e69676558993b103d68b19e08';out=[]
def g(*args):
 q=subprocess.run(['git','-C',r,*args],capture_output=True,text=True);out.append({'argv':['git','-C',r,*args],'exit':q.returncode,'stdout':q.stdout,'stderr':q.stderr});return q.stdout
for a in ['74afd5575','a40d63a19','50786828f','77bcc82e1','1109eeea1','cbb538fee','798f7b229','2e5a14a81','332394f16','18f7ce2b7','7bf5f760d','7e5b519bf']:
 sha=g('rev-parse',a).strip()
 if len(sha)==40:
  g('merge-base','--is-ancestor',sha,i);g('merge-base','--is-ancestor',sha,p)
for rev in [i,p]:
 g('ls-tree',rev,'packages/engine/runtime/runtime-src')
 g('show',rev+':multiplatform-upstream-catchup-2026-11-impl.md')
for path in ['crates/pdfium/src/fonts.rs','crates/pdfium/src/font_program.rs','crates/pdfium/src/document_fonts.rs','crates/epdf-engine/src/js_number.rs']:
 g('diff','--numstat',i,p,'--',path)
print(json.dumps(out,indent=2))
