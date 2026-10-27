"""C11 full occurrence accounting; broad BASE preservation is a one-time audit."""
import json,pathlib,hashlib,subprocess,collections,re,sys
H=pathlib.Path(__file__).resolve().parent;ROOT=H.parents[3];BASE='e4f60a19960b68c0e7ec60631bf30d102dbccf33'
def sha(b):return hashlib.sha256(b).hexdigest()
def digest(v):return sha(json.dumps(v,ensure_ascii=False,separators=(',',':')).encode())
def load(p):return json.loads((H/p).read_text())
def write(p,v):(H/p).write_text(json.dumps(v,ensure_ascii=False,indent=2)+'\n')
def diff(a,b,p='$'):
 if a==b:return []
 if type(a)!=type(b)or not isinstance(a,(dict,list)):return[p]
 if isinstance(a,list):return[q for i in range(max(len(a),len(b)))for q in(diff(a[i],b[i],p+f'[{i}]')if i<len(a)and i<len(b)else[p+f'[{i}]'])]
 return[q for k in dict.fromkeys([*a,*b])for q in(diff(a[k],b[k],p+'.'+k)if k in a and k in b else[p+'.'+k])]
frozen=ROOT/'.superpowers/sdd/multiplatform-upstream-catchup/task-12b2c-before';old=load('original-runtime.json');cur=load('current-inputs-runtime.json')
assert len(old)==76 and len(cur)==95
assert old==[r for r in json.loads((frozen/'inputs-runtime.json').read_text())if r['source']=='view-manager']
for archive,path in [('original-view-manager.ts.txt','tooling/plugin-conformance/src/suites/view-manager.ts'),('original-commands-current.test.ts.txt','tooling/plugin-conformance/test/commands-current.test.ts')]:
 assert (H/archive).read_bytes()==subprocess.check_output(['git','show',BASE+':'+path],cwd=ROOT)
assert(H/'original-view-manager.ts.txt').read_bytes()==(frozen/'view-manager.ts.txt').read_bytes()
renames={'unknown-action-is-ignored':'next-pane-id','set-focused-same-is-a-new-state':'set-focused-same-is-ignored','remove-doc-absent-is-a-new-state':'remove-doc-absent-is-ignored','add-takes-the-first-holder-in-property-order':'add-takes-the-first-holder-in-pane-order','a-kernel-refuses-the-shipping-plugin':'a-kernel-installs-the-shipping-plugin'}
modelPath='packages/plugin/view-manager/src/model.ts';controllerPath='packages/plugin/view-manager/src/controller.ts'
def source(path,needle):
 lines=(ROOT/path).read_text().splitlines();return {'path':path,'line':next(i+1 for i,line in enumerate(lines)if needle in line),'symbol':needle,'sha256':sha((ROOT/path).read_bytes())}
def shipping_sources(current):
 if current['kind']=='function':
  target=current['input']['fn'];return [source(modelPath,('export function ' if target not in ['initialViewManagerState','nextPaneId','paneOfDocument']else 'export const ')+target)]
 return [source(controllerPath,'export function createViewManagerController'),source(controllerPath,'connect()'),source('packages/plugin/view-manager/src/view-manager.plugin.ts','export const viewManagerPlugin')]
mappings=[];used=set()
for i,o in enumerate(old):
 name=renames.get(o['name'],o['name']);matches=[(j,c)for j,c in enumerate(cur)if c['file']==o['file']and c['name']==name];assert len(matches)==1
 j,c=matches[0];assert j not in used;used.add(j)
 whole=o['name']=='unknown-action-is-ignored';partial=o['input'].get('fn')=='viewManagerReducer'and not whole
 reason='The removed arbitrary message dispatcher has no current shipping equivalent; retain original and replace active row with nextPaneId selector coverage, not semantic equivalence.'if whole else'Retire private action envelope and same/next lens; replay actual typed model transition and positional arguments. Reference identity is asserted directly.'if partial else None
 removed=None
 if o['name']in ['create','create-on-empty']:
  removed='CREATE_PANE accepted arbitrary supplied pane identity, contents and replacement at an existing key. Current createPane owns generated nextPaneId, empty contents, seq increment and insertion position. Only creation/focus/order intent survives; arbitrary injection/replacement is retired, never emulated.'
 if o['name']=='reorder':
  removed='REORDER_PANES accepted wholesale arbitrary order replacement (including foreign/duplicate/missing ids). Current movePane takes one existing id and positional index. This row moves pane-3 to0, preserving the valid permutation outcome; arbitrary whole-order replacement is retired, never emulated.'
 if o['name']=='a-kernel-refuses-the-shipping-plugin':
  reason='BASE runtime already installs successfully: stale refusal narrative and obsolete context proxy are removed. Direct shipping definePlugin workspace registration is now independently asserted; no current refusal is manufactured.'
 mappings.append({'originalOccurrence':i,'originalFileIndex':sum(r['file']==o['file']for r in old[:i]),'originalName':o['name'],'originalFile':o['file'],'originalInput':o['input'],'originalInputSha256':o['inputSha256'],'originalOutcome':o.get('outcome',o.get('result',o.get('runnerException'))),'originalOutcomeSha256':digest(o.get('outcome',o.get('result',o.get('runnerException')))),'currentFile':c['file'],'currentIndex':c['index'],'currentName':c['name'],'currentInput':c['input'],'currentInputSha256':c['inputSha256'],'currentOutcome':c.get('outcome',c.get('result')),'currentOutcomeSha256':digest(c.get('outcome',c.get('result'))),'changedInputPaths':diff(o['input'],c['input']),'disposition':'whole-retired-with-replacement'if whole else'partial-retirement'if partial else'preserved-current-contract','removedPaneMessageContract':removed,'reason':reason,'sourceEvidence':'source-evidence.json','shippingSources':shipping_sources(c),'retirementSource':{'commit':'4fc1a32af6d6ece9022a08139ac721e5c6fa8411','parentArchive':'before-named-transitions.ts.txt','parentPath':modelPath,'patch':'named-transitions.patch.txt','removedDiscriminant':'CREATE_PANE'if o['name']in ['create','create-on-empty']else'REORDER_PANES'if o['name']=='reorder'else'dispatch action envelope'}if partial or whole else None})
additions=[dict(c,shippingSources=shipping_sources(c),reason='Current generated ids/sequence, positional move boundaries, rename/selectors, holder order or pending-ticket/split-placement coverage.')for j,c in enumerate(cur)if j not in used]
summary={'original':76,'originalPure':64,'originalLive':12,'current':95,'currentPure':82,'currentLive':13,'mapped':len(mappings),'wholeRetirements':1,'survivingOriginals':75,'replacementRows':1,'totalNewRows':20,'partialRetirements':sum(m['disposition']=='partial-retirement'for m in mappings),'removedPaneMessageOccurrences':sum(bool(m['removedPaneMessageContract'])for m in mappings),'additions':len(additions),'changedInputs':sum(bool(m['changedInputPaths'])for m in mappings)}
write('accounting.json',{'summary':summary,'arithmetic':'76 originals - 1 whole retirement + 1 mapped replacement + 19 additional rows =95 current','mappings':mappings,'replacements':[m for m in mappings if m['disposition']=='whole-retired-with-replacement'],'additions':additions})
commits={label:subprocess.check_output(['git','rev-parse',ref],cwd=ROOT,text=True).strip()for label,ref in {'namedTransitions':'4fc1a32af','currentContract':'cdf4586c7'}.items()}
paths=['packages/plugin/view-manager/src/'+p for p in ['model.ts','controller.ts','contract.ts','view-manager.plugin.ts','state.ts','token.ts','index.ts']]+['packages/plugin/view-manager/test/'+p for p in ['controller.test.ts','partition.test.ts']]
write('source-evidence.json',{'base':BASE,'commits':commits,'owners':[{'path':p,'sha256':sha((ROOT/p).read_bytes())}for p in paths],'contracts':{'construction':'view-manager.plugin.ts: workspace definePlugin state initialViewManagerState/create createViewManagerController; no proxy or old context spread','generatedCreation':'model.ts createPane/nextPaneId: monotonic seq, generated pane-N, empty pane, insertion index and focus; contract.ts CreatePaneOptions has documentIds only, no injected pane/id','paneMovement':'model.ts movePane filters one id and clamps insertion index; controller passes positional id/toIndex; no whole-order replacement','partition':'paneOfDocument walks explicit state.order; addDocument removes from first holder and retains target active unless null; between-pane movement focuses destination and activates moved doc','removal':'removePane model drops pane and uses last remaining focus; capability first moves each doc to explicit/focused/last valid target; invalid/self target drops rather than emulates a different policy','renameReconcile':'renameDocument replaces previous id in place/active slot; connect detects equal-length disjoint replacement positions, renames before reconcile; reconcile prunes, validates active and appends unassigned in registry order using preferred/focused/first/new fallback','identity':'same-focus/same-active/absent-remove operations return identical state; move same index is still a new state; memoByKey keeps unaffected pane reference and list is stable until panes/order changes','events':'created in next order, removed in previous order, focus next, document moved in previous+next object enumeration; same-pane tab movement has no document-moved event; throwing listeners report and siblings still run; teardown unsubscribes watch','retirement':'3 removed pane-message occurrences: 2 CREATE_PANE arbitrary supplied panes and1 REORDER_PANES full order. Explicit partial scope retained with evidence; unknown dispatcher is a whole retirement/replacement. Full originals remain immutable.'}})
drift=[r for r in json.loads((frozen/'task11-current-drift.json').read_text())if r['source']=='view-manager'];assert not drift;write('task11-current-drift.json',drift)
counts=collections.Counter();negative=[]
for c in cur:
 if c['kind']=='function':assert'returned'in c['outcome'];continue
 r=c['result'];assert not r['unmatched'];counts['scenarios']+=1
 for i,t in enumerate(r['trace']):
  counts['steps']+=1;counts['callbacks']+=len(t['callbacks']);counts['settlements']+=len(t['settled'])
  for channel,values in [('stepThrows',[t['outcome']['threw']]if'threw'in t.get('outcome',{})else[]),('reports',t['reports']),('rejections',[s['rejected']for s in t['settled']if'rejected'in s])]:
   for value in values:counts[channel]+=1;negative.append({'name':c['name'],'step':i,'channel':channel,'value':value,'source':source('packages/core/main/src/event-hook.ts','listener(event)')if channel=='reports'else source('packages/core/main/src/kernel.ts','${what} on a ${status} kernel')if channel=='stepThrows'else source('packages/core/main/src/kernel.ts','openFailed')})
write('negative-channels.json',negative);write('runtime-summary.json',dict(counts,pure=82,pureThrows=0,runnerExceptions=0,unmatched=0))
if '--reconcile-only'not in sys.argv:
 exempt={'tooling/plugin-conformance/src/suites/view-manager.ts','tooling/plugin-conformance/test/commands-current.test.ts'};files=[]
 for p in subprocess.check_output(['git','ls-tree','-r','--name-only',BASE,'tooling/plugin-conformance'],cwd=ROOT,text=True).splitlines():
  if p in exempt:continue
  b=subprocess.check_output(['git','show',BASE+':'+p],cwd=ROOT);assert b==(ROOT/p).read_bytes(),p;files.append({'path':p,'sha256':sha(b)})
 write('preservation.json',{'base':BASE,'purpose':'one-time audit only; never future suite guards','exempt':sorted(exempt),'preservedCount':len(files),'files':files})
allocation=collections.Counter(re.findall(r'^(.+?)\(\d+,\d+\): error TS',(H/'full-typecheck.out').read_text(),re.M));assert not any('view-manager'in p for p in allocation)
write('diagnostics.json',{'total':sum(allocation.values()),'allocation':dict(allocation),'owned':0})
write('integrity.json',{p.name:sha(p.read_bytes())for p in H.iterdir()if p.suffix in ['.json','.txt']and p.name!='integrity.json'})
print(json.dumps({'summary':summary,'runtime':load('runtime-summary.json'),'diagnostics':load('diagnostics.json'),'preserved':load('preservation.json')['preservedCount']},indent=2))
