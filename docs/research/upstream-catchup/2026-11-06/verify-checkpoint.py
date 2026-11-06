import subprocess,json,os,datetime,concurrent.futures,hashlib
from pathlib import Path
root=Path('/workspaces/catchup-2026-11-06')
logs=root/'.superpowers/sdd/upstream-catchup-2026-11-06'
env={**os.environ,'NPM_TOKEN':'','CARGO_BUILD_JOBS':'1','PDF_RUNTIME_LIB_DIR':'/workspaces/catchup-final-runtime-artifacts','EMBEDPDF_FORK_TARGET':'linux-arm64','LD_LIBRARY_PATH':'/workspaces/catchup-final-runtime-artifacts/linux-arm64/lib'}
checks=[
 ('rust-ports',['cargo','test','-p','epdf-engine','--lib','--test','action_reader','--test','attachment_reader','--test','charmap','--test','text_corpus','--test','annotation_line_breaks','--test','annotation_stroke_dash','--test','annotation_write','--test','write_transactions'],root,True),
 ('boundaries',['pnpm','check:plugin-boundaries'],root,True),
 ('boundary-abi-tests',['node','--test','tooling/build/test/plugin-boundaries.test.mjs','packages/engine/runtime/test/generate-functions.test.mjs'],root,True),
 ('coverage-tests',['pnpm','--filter','@embedpdf/tooling-engine-conformance','test'],root,True),
 ('text-vectors',['pnpm','--filter','@embedpdf/tooling-engine-conformance','vectors:check'],root,True),
 ('charmap-vectors',['pnpm','--filter','@embedpdf/tooling-engine-conformance','charmap:check'],root,True),
 ('attachment-vectors',['pnpm','--filter','@embedpdf/tooling-engine-conformance','attachments:check'],root,True),
 ('newline-vectors',['pnpm','--filter','@embedpdf/tooling-engine-conformance','annotation-line-breaks:check'],root,True),
 ('xyz-wasm',['pnpm','exec','tsx','--conditions=development','src/generate-metadata-vectors.ts','--xyz-only','--check'],root/'tooling/engine-conformance',True),
 ('xyz-native',['pnpm','exec','tsx','--conditions=development','src/generate-metadata-vectors.ts','--xyz-only','--runtime=native','--check'],root/'tooling/engine-conformance',True),
 ('stroke-create',['pnpm','exec','tsx','--conditions=development','src/generate-annotation-create-vectors.ts','--stroke-dash-only','--check'],root/'tooling/engine-conformance',True),
 ('stroke-update',['pnpm','exec','tsx','--conditions=development','src/generate-annotation-update-vectors.ts','--stroke-dash-only','--check'],root/'tooling/engine-conformance',True),
 ('runtime-tests',['node','--test',*[str(p) for p in sorted((root/'packages/engine/runtime/test').glob('*.test.mjs'))]],Path('/workspaces/catchup-high-heap-fixtures'),True),
 ('remaining-typecheck',['pnpm','--filter','@embedpdf/tooling-engine-conformance','typecheck'],root,False),
]
def run(item):
 name,args,cwd,required=item
 log=logs/('checkpoint-'+name+'.log')
 with log.open('w') as f:r=subprocess.run(args,cwd=cwd,env=env,stdout=f,stderr=subprocess.STDOUT)
 return {'name':name,'command':args,'cwd':str(cwd),'exit':r.returncode,'requiredForInitialSlice':required,'log':str(log),'tail':log.read_text()[-6000:]}
with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:results=list(pool.map(run,checks))
receipt={'verifiedUTC':datetime.datetime.now(datetime.timezone.utc).isoformat(),'source':subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),'viewerTarget':'8297bad8d388eb9c70324c22cb001c59248f95f4','runtimeFork':'1e214deb2869d9233e573666d239bc32ce73f186','runtimeTarget':'4346c5131cd009c4086fdce13bde7fd43fbd4ce0','initialSlicePassed':all(x['exit']==0 for x in results if x['requiredForInitialSlice']),'wholeCatchupComplete':False,'runtimeWasmScope':'Matching PDFium fork TypeScript oracle only; final combined Rust browser payload and platform package acceptance remain pending','checks':results}
(logs/'checkpoint-results.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps({k:v for k,v in receipt.items() if k!='checks'},indent=2))
for x in results:print(x['name'],x['exit'])
raise SystemExit(0 if receipt['initialSlicePassed'] else 1)
