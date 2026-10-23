"""Read-only comparison to the reviewed C6 implementation, then write fix evidence."""
import hashlib,json,pathlib,subprocess
ROOT=pathlib.Path(__file__).resolve().parents[4]
H=pathlib.Path(__file__).resolve().parent
FIX_BASE='4b3ad039e3fd8bdac2569c38144fe67c561c6772'
PREFIX='tooling/plugin-conformance/historical/task-12b2c6/'
changed={'set-an-empty-page','set-another-page','set-replaces-a-page'}
sha=lambda b:hashlib.sha256(b).hexdigest()
def blob(path):return subprocess.check_output(['git','show',f'{FIX_BASE}:{path}'],cwd=ROOT)
def before(name):return json.loads(blob(PREFIX+name))
def now(name):return json.loads((H/name).read_text())
old=before('current-inputs-runtime.json');new=now('current-inputs-runtime.json')
assert len(old)==len(new)==37
unchanged=[];changes=[]
for a,b in zip(old,new):
 assert (a['file'],a['name'],a['kind'],a['index'])==(b['file'],b['name'],b['kind'],b['index'])
 if a['name'] in changed:
  assert a!=b;changes.append({'name':a['name'],'oldInputSha256':a['inputSha256'],'currentInputSha256':b['inputSha256'],'oldSteps':len(a['result']['trace']),'currentSteps':len(b['result']['trace'])})
 else:
  assert a==b,a['name'];unchanged.append({'file':a['file'],'name':a['name'],'inputSha256':a['inputSha256'],'fullInputAndOutcomeExact':True})
assert len(unchanged)==34 and len(changes)==3
oldm=before('accounting.json')['mappings'];newm=now('accounting.json')['mappings']
for a,b in zip(oldm,newm):
 assert a['name']==b['name']
 for field in ['originalOccurrence','originalSuiteIndex','originalIndexInSuite','file','name','originalKind','originalInput','originalInputSha256','originalOutcome']:
  assert a[field]==b[field],(a['name'],field)
 if a['name'] not in changed:assert a==b,a['name']
assert before('negative-channels.json')==now('negative-channels.json')
assert before('returned-refusals.json')==now('returned-refusals.json')
allowed={PREFIX+p for p in ['account.py','accounting.json','current-inputs-runtime.json','integrity.json','runtime-summary.json','source-evidence.json']}
allowed|={'tooling/plugin-conformance/src/suites/link.ts','tooling/plugin-conformance/test/link-current.test.ts','.superpowers/sdd/multiplatform-upstream-catchup/task-12b2c6-report.md'}
paths=subprocess.check_output(['git','ls-tree','-r','--name-only',FIX_BASE,'tooling/plugin-conformance'],cwd=ROOT,text=True).splitlines()
preserved=[]
for path in paths:
 if path in allowed:continue
 data=(ROOT/path).read_bytes();assert data==blob(path),path
 preserved.append({'path':path,'sha256':sha(data)})
originals={p:sha(blob(PREFIX+p)) for p in ['original-link.ts.txt','original-runtime.json','base-link.ts.txt','base-acrojs-current.test.ts.txt','base-actions-current.test.ts.txt','base-commands-current.test.ts.txt']}
for p,want in originals.items():assert sha((H/p).read_bytes())==want
# Shipping source evidence and every unrelated retirement remain exact.
a=before('source-evidence.json');b=now('source-evidence.json');assert a['currentOwners']==b['currentOwners']
for name,value in a['partialRetirements'].items():
 if name!='set-an-empty-page':assert value==b['partialRetirements'][name]
out={'fixBase':FIX_BASE,'changedCases':changes,'other34CompleteInputsAndOutcomes':unchanged,'all29OriginalDefinitionsInputsAndOutcomesExact':True,'other26MappingsExact':True,'negativeChannelsExact':True,'returnedRefusalsExact':True,'originalAuthorities':originals,'preservedPrivateToolingCount':len(preserved),'preservedPrivateToolingFiles':preserved,'allowedExistingPaths':sorted(allowed),'prior239':'Complete actions160/commands79 inputs/outcomes replayed by fix1-green.out test; their authority/source files exact in this audit.'}
(H/'fix1-preservation.json').write_text(json.dumps(out,indent=2)+'\n')
print(json.dumps({k:v for k,v in out.items() if k not in ['preservedPrivateToolingFiles','other34CompleteInputsAndOutcomes','allowedExistingPaths']},indent=2))
