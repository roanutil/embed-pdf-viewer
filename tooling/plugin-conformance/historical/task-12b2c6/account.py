"""Reconcile immutable original29 authority with current cases. No runtime capture."""
import collections,hashlib,json,pathlib,subprocess
ROOT=pathlib.Path(__file__).resolve().parents[4]
H=pathlib.Path(__file__).resolve().parent
BASE='df620089ce9ca46eba26e39ad13573af88bcd23e'
sha=lambda b:hashlib.sha256(b).hexdigest()
def write(n,v): (H/n).write_text(json.dumps(v,indent=2,ensure_ascii=False)+'\n')
def load(n):return json.loads((H/n).read_text())
def blob(p):return subprocess.check_output(['git','show',f'{BASE}:{p}'],cwd=ROOT)
def hash_json(v):return sha(json.dumps(v,separators=(',',':'),ensure_ascii=False).encode())
original=load('original-runtime.json');current=load('current-inputs-runtime.json')
assert len(original)==29 and len(current)==37
frozen=ROOT/'.superpowers/sdd/multiplatform-upstream-catchup/task-12b2c-before'
assert (H/'original-link.ts.txt').read_bytes()==(frozen/'link.ts.txt').read_bytes()
assert (H/'original-runtime.json').read_bytes()==blob('tooling/plugin-conformance/historical/task-12b2c2/link-runtime.json')
owned=['tooling/plugin-conformance/src/suites/link.ts','tooling/plugin-conformance/test/acrojs-current.test.ts','tooling/plugin-conformance/test/actions-current.test.ts','tooling/plugin-conformance/test/commands-current.test.ts']
original_sources=[]
for p in owned:
 data=blob(p);name='base-'+pathlib.Path(p).name+'.txt';(H/name).write_bytes(data)
 original_sources.append({'path':p,'archive':name,'sha256':sha(data)})
write('base-source-authority.json',original_sources)
# All previously tracked private tooling except exactly the active four files.
paths=subprocess.check_output(['git','ls-tree','-r','--name-only',BASE,'tooling/plugin-conformance'],cwd=ROOT,text=True).splitlines()
files=[]
for p in paths:
 if p in owned:continue
 data=blob(p);assert (ROOT/p).read_bytes()==data,p
 files.append({'path':p,'sha256':sha(data)})
write('preservation.json',{'base':BASE,'exemptions':owned,'preservedCount':len(files),'files':files})
owners={
 'controller':'packages/plugin/link/src/controller.ts','source':'packages/plugin/link/src/source.ts',
 'contract':'packages/plugin/link/src/contract.ts','connect':'packages/plugin/link/src/connect.ts',
 'mirror':'packages/core/main/src/page-mirror.ts','eventHook':'packages/core/main/src/event-hook.ts',
 'destinations':'packages/engine/core/src/dto/PdfDestination.ts','annotation':'packages/plugin/annotation/src/controller.ts',
 'pageRef':'packages/engine/core/src/identity/PageRef.ts','annotationRef':'packages/engine/core/src/identity/AnnotationRef.ts',
 'changes':'packages/engine/core/src/mutation/Change.ts','actions':'packages/plugin/actions/src/contract.ts',
}
owner_evidence=[]
for name,p in owners.items():
 data=(ROOT/p).read_bytes();owner_evidence.append({'owner':name,'path':p,'sha256':sha(data)})
# Exact historical shipping source evidence for each retired component.
legacy_paths=['model.ts','controller.ts','source.ts','contract.ts']
for p in legacy_paths:
 result=subprocess.run(['git','show',f'4fc1a32af^:packages/plugin/link/src/{p}'],cwd=ROOT,capture_output=True)
 if result.returncode==0:(H/('pre-4fc1a32af-'+p+'.txt')).write_bytes(result.stdout)
retirements={
 'viewer-only-page-cache':'Old loadLinksPage PDF rect conversion, nm/index refs and null/omitted target distinction retire: current linksOf consumes complete page-space Annotation and annotationKey refs. Missing page now rejects not-found, rather than silently no-op.',
 'viewer-only-malformed-lists':'Whole malformed-record subcontract retires: incomplete flags/ref/rect/index, invalid action trees and NaN PDF edge conversion are outside Annotation DTO. Retain six distinct engine read refusal/error/retry paths; original full malformed definitions remain immutable.',
 'viewer-only-refetch-on-events':'annotation.moved and empty-move payload retire; current annotations.updated/reordered facts replace them. Even empty order refreshes. Event page is current fact page, and uncached pages do not fetch.',
 'annotation-backed-source':'Old folded reducer hydration shape retires; current whole-document Annotation snapshot plus confirmed current apply results/events owns the same links. Add explicit local update/delete operations and remote delete.',
 'activate-reveals-smoothly':'Old link-owned forced-smooth reveal retires; controller delegates goToDestination to the stage configured scrollBehavior. Explicit current smooth plus host frames preserves animation coverage.',
 'activate-destination-that-cannot-invert':'Old link-owned PDF geometry inversion and its singular-userUnit throw retire. PageDestination is already page space; current stage reveal succeeds and emits both callbacks.',
 'a-throwing-listener-throws-out-of-activate':'The original case text expected throw propagation but was already stale: pre-4fc1a32af controller.ts uses an isolated createEventHook and console error reporter. Current ctx.events owns isolation/reporting, reports exactly once through ctx.report after reveal, and returns successful activation. Retire only stale expectation/console shim, not an actual former throwing shipping contract.',
 'list-all-links-annotation-backed':'Invalid-page isLoaded no longer unconditionally true with annotation present; ctx.getPage validates first. Existing page remains ready without reads.',
 'labels-and-resolution':'Old resolved reveal options and missing-page destination retire; current resolveLinkTarget returns destination only for an existing page and reports absent target verbatim.',
}
for o in original:
 if o['kind']=='function':retirements[o['name']]='Removed model.ts initialLinkState/linkReducer and public state identity/changed semantics retire at 4fc1a32af. Current controller delegates cache to pageMirror: initial idle, ensureLoaded/reload pages, and pages.deleted epoch invalidation/drop. Empty/absent drop keeps no cache; cached empty page deletion becomes idle. No copied reducer remains.'
retirements['set-an-empty-page'] += ' Original objectNumber 0 is not a valid PDF page object; retain the original loaded page1/item1 and cached-empty page3, then add actual page2 as empty and observe all three ready/loaded states.'
retirements['set-another-page'] += ' Original action page2 carried an item ref naming page1; current engine Annotation/ref consistently name page2. Original bounds, URI, activate u and hover-enter payload survive.'
# Enumerate every changed input leaf, including additions and retirements.
def diff(a,b,p='$'):
 if type(a)!=type(b):return [p]
 if isinstance(a,dict):
  out=[]
  for k in sorted(a.keys()|b.keys()):out+= [p+'.'+k] if k not in a or k not in b else diff(a[k],b[k],p+'.'+k)
  return out
 if isinstance(a,list):
  out=[]
  for i in range(max(len(a),len(b))):out += [f'{p}[{i}]'] if i>=len(a) or i>=len(b) else diff(a[i],b[i],f'{p}[{i}]')
  return out
 return [] if a==b else [p]
mappings=[]
for i,o in enumerate(original):
 c=next(c for c in current if c['name']==o['name'])
 assert o['inputSha256']==hash_json(o['input']);assert c['inputSha256']==hash_json(c['input'])
 mappings.append({'originalOccurrence':i,'originalSuiteIndex':0 if o['kind']=='scenario' else 1,'originalIndexInSuite':i if o['kind']=='scenario' else i-20,'file':o['file'],'name':o['name'],'originalKind':o['kind'],'originalInput':o['input'],'originalInputSha256':o['inputSha256'],'originalOutcome':{k:o[k] for k in ['result','outcome','runnerException'] if k in o},'currentFile':c['file'],'currentKind':c['kind'],'currentName':c['name'],'currentIndex':c['index'],'currentInputSha256':c['inputSha256'],'changedInputPaths':diff(o['input'],c['input']),'partialRetirement':retirements.get(o['name']),'currentOwners':['mirror','controller'] if o['kind']=='function' else ['controller','source','contract','connect','annotation','actions','eventHook','changes']})
additions=[{'file':c['file'],'name':c['name'],'inputSha256':c['inputSha256'],'reason':'Current shared shipping resolveLinkTarget branch; serialized target and known current pages are actual function arguments.'} for c in current if not any(o['name']==c['name'] for o in original)]
summary={'original':29,'current':37,'mapped':29,'wholeRetirements':0,'partialRetirements':sum(bool(m['partialRetirement']) for m in mappings),'additions':len(additions),'changedInputs':sum(bool(m['changedInputPaths']) for m in mappings)}
write('accounting.json',{'base':BASE,'originalAuthority':'immutable C2 link-runtime.json; frozen C source link.ts.txt','summary':summary,'mappings':mappings,'additions':additions})
write('source-evidence.json',{'removedModelCommit':'4fc1a32af6d6ece9022a08139ac721e5c6fa8411','currentOwners':owner_evidence,'partialRetirements':retirements})
def stats(rows):
 out=collections.Counter()
 for c in rows:
  out['cases']+=1
  if 'runnerException'in c:out['runnerExceptions']+=1;continue
  if 'outcome'in c:
   out['pure']+=1;out['pureThrows']+='threw'in c['outcome'];continue
  r=c['result'];out['scenario']+=1;out['steps']+=len(r['trace']);out['unmatched']+=len(r['unmatched'])
  for t in r['trace']:
   out['stepThrows']+='threw'in t.get('outcome',{});out['settlements']+=len(t.get('settled',[]));out['rejections']+=sum('rejected'in v for v in t.get('settled',[]));out['reports']+=len(t.get('reports',[]));out['callbacks']+=len(t.get('callbacks',[]))
 return dict(out)
write('runtime-summary.json',{'original':stats(original),'current':stats(current)})
channels=[]
for c in current:
 for step,t in enumerate(c.get('result',{}).get('trace',[])):
  if 'threw'in t.get('outcome',{}):channels.append({'name':c['name'],'step':step,'channel':'threw','value':t['outcome']['threw']})
  for v in t['settled']:
   if 'rejected'in v:channels.append({'name':c['name'],'step':step,'channel':'rejected','value':v['rejected']})
   r=v.get('resolved')
   if isinstance(r,dict) and r.get('diagnostics'):channels.append({'name':c['name'],'step':step,'channel':'nested action result','value':r})
  for r in t['reports']:channels.append({'name':c['name'],'step':step,'channel':'reports','value':r})
write('negative-channels.json',channels)
returned=[]
for c in current:
 if 'outcome' in c:
  value=c['outcome'].get('returned')
  if isinstance(value,dict) and value.get('kind')=='reported':returned.append({'name':c['name'],'channel':'pure.returned','value':value})
 for step,t in enumerate(c.get('result',{}).get('trace',[])):
  outcome=t.get('outcome',{})
  if 'returned' not in outcome:continue
  value=outcome['returned']
  if value is None or (isinstance(value,dict) and (value.get('outcome')=='reported' or value.get('kind')=='reported')):
   returned.append({'name':c['name'],'step':step,'channel':'returned absence/refusal','value':value})
write('returned-refusals.json',returned)
write('direct-test-contracts.json',{'uriOpeners':'latest opener wins; false reports target; removing stale first never removes second; double unsubscribe inert; absent opener yields uri','listenerIsolation':'listener-isolated Error reported once; sibling sees exact target/activation; unsubscribed observer never called','lifetime':'close aborts guarded handle; pageMirror suppresses instance-closed then resolves ensureLoaded; late engine settlement never emits loaded; subscribers 0','callerCancellation':{'name':'PluginError','code':'operation-cancelled','capability':'link','message':'operation cancelled'},'sharedReadAfterCancellation':'caller signal rejects only caller; other waiter sees ready and obj:1 after explicit engine settlement'})

drift=json.loads((frozen/'task11-current-drift.json').read_text())
write('task11-current-drift.json',[r for r in drift if 'link' in json.dumps(r)])
write('integrity.json',{p.name:sha(p.read_bytes()) for p in H.iterdir() if p.suffix in ['.json','.txt'] and p.name!='integrity.json'})
print(json.dumps({'summary':summary,'runtime':load('runtime-summary.json'),'preserved':len(files)},indent=2))
