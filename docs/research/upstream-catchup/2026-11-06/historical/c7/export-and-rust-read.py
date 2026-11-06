import subprocess,re,json,hashlib
records=[]
def g(d,*args):
 p=subprocess.run(['git','--git-dir',d,*args],capture_output=True,text=True);records.append({'argv':p.args,'exit':p.returncode,'stdout':p.stdout,'stderr':p.stderr});return p.stdout
v='/tmp/embedpdf-c7-viewer-metadata.git';r='/tmp/embedpdf-c7-runtime-metadata.git';old='f28ed4ea4aa46c8e69676558993b103d68b19e08';variants={}
for sha in ['3c55b7a1c0bf956231392a573e87f28dd6d6667d','f1ba46f0dc3dc5f9b055a7311dda0c13ecc0f185','eeac787c02fac7b40c74c91bbff0ffb5f59ff97e']:
 files=g(r,'ls-tree','-r','--name-only',sha,'public').splitlines();functions={}
 for path in files:
  if not path.endswith('.h'):continue
  text=g(r,'show',sha+':'+path)
  for match in re.finditer(r'FPDF_EXPORT\s+([^;{}]+?\bFPDF_CALLCONV\s+(\w+)\s*\([^;{}]*\))\s*;',text,re.S):functions[match.group(2)]=re.sub(r'\s+',' ',match.group(1)).strip()
 variants[sha]=functions
oldexports=variants['3c55b7a1c0bf956231392a573e87f28dd6d6667d'];summary={}
for sha,cur in variants.items():summary[sha]={'count':len(cur),'added':{k:cur[k] for k in cur.keys()-oldexports.keys()},'removed':list(oldexports.keys()-cur.keys()),'signatureChanged':{k:{'old':oldexports[k],'new':cur[k]} for k in oldexports.keys()&cur.keys() if oldexports[k]!=cur[k]}}
for path in ['crates/epdf-engine/src/features/actions/destination.rs','crates/pdfium/src/action.rs','crates/epdf-engine/src/features/annotation/rich_text.rs']:
 g(v,'show',old+':'+path)
g(v,'ls-tree','-r','--name-only',old,'crates/epdf-engine/src/features/annotation')
g(v,'grep','-n','-E','DashArray|dash_array|GeometryStyle|stroke_color|change_kind|can_change_kind|set_subtype',old,'--','crates/epdf-engine/src','crates/pdfium/src')
print(json.dumps({'exportSummary':summary,'commands':records},indent=2))
