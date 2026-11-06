import subprocess,json
out=[]
def g(d,*args):
 p=subprocess.run(['git','--git-dir',d,*args],capture_output=True,text=True);out.append({'argv':p.args,'exit':p.returncode,'stdout':p.stdout,'stderr':p.stderr});return p.stdout
v='/tmp/embedpdf-c7-viewer-metadata.git';r='/tmp/embedpdf-c7-runtime-metadata.git';old='f28ed4ea4aa46c8e69676558993b103d68b19e08';new='4ad8986099314b9587df423c802db103a21d665d';rold='3c55b7a1c0bf956231392a573e87f28dd6d6667d';rnew='f1ba46f0dc3dc5f9b055a7311dda0c13ecc0f185'
for path in ['packages/engine/runtime/src/core/pdf-functions.generated.ts','packages/engine/services/src/features/destinations/readDestination.ts','packages/engine/core/src/dto/DocumentFonts.ts','packages/core/fonts/src/standard/css.ts','packages/engine/core/src/annotation/siblings.ts','packages/engine/core/src/annotation/resolve/comment.ts','packages/engine/core/src/annotation/resolve/dash.ts','packages/engine/core/src/annotation/resolve/applyAnnotationPatch.ts','packages/engine/services/src/features/annotations/AnnotationMutator.ts','packages/engine/services/src/features/annotations/internal/write/writeStyle.ts','packages/engine/services/src/features/annotations/internal/richTextWire.ts','packages/engine/runtime/engine-runtime-build.json','packages/engine/core/src/wire/schemas.ts']:
 g(v,'diff',old,new,'--',path)
for d,a,b in [(v,old,new),(r,rold,rnew)]:
 base=g(d,'merge-base',a,b).strip();g(d,'diff','--name-only',base,a);g(d,'diff','--name-only',base,b);g(d,'cherry',a,b)
g(v,'grep','-n','-E','SetSubtype|SetRichText|standardFallback|standard_fallback|bezier|ink_curves',old,'--','crates')
g(v,'grep','-n','-E','read_destination|readDestination|destination_xyz|XYZ',old,'--','crates/pdfium/src','crates/epdf-engine/src')
g(r,'diff',rold,rnew,'--','public/epdf_annot.h','DEPS','.gn','scripts/embedpdf-runtime/build-target.sh')
g(r,'diff',rnew,'eeac787c02fac7b40c74c91bbff0ffb5f59ff97e','--','public/epdf_annot.h','core/fpdfdoc/cpdf_richtextparser.cpp')
print(json.dumps(out,indent=2))
