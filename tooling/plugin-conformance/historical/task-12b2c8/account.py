"""One-time C8 reconciliation; preserved manifests are audit evidence, never future source guards."""
import pathlib,json,hashlib,subprocess,collections,sys
ROOT=pathlib.Path(__file__).resolve().parents[4];H=pathlib.Path(__file__).resolve().parent
BASE='fe4e8b81793d9ea436f357a41e4de7c0b19b2eaa'
def sha(v):return hashlib.sha256(v).hexdigest()
def digest(v):return sha(json.dumps(v,separators=(',',':'),ensure_ascii=False).encode())
def write(p,v):(H/p).write_text(json.dumps(v,indent=2,ensure_ascii=False)+'\n')
def load(p):return json.loads((H/p).read_text())
def blob(p,ref=BASE):return subprocess.check_output(['git','show',ref+':'+p],cwd=ROOT)
frozen=ROOT/'.superpowers/sdd/multiplatform-upstream-catchup/task-12b2c-before'
old=load('original-runtime.json');current=load('current-inputs-runtime.json');assert len(old)==19 and len(current)==19
assert old==[r for r in json.loads((frozen/'inputs-runtime.json').read_text()) if r['source']=='page-edit']
assert (H/'original-page-edit.ts.txt').read_bytes()==(frozen/'page-edit.ts.txt').read_bytes()
paths=['packages/plugin/page-edit/src/controller.ts','packages/plugin/page-edit/src/contract.ts','packages/plugin/page-edit/src/model.ts','packages/core/main/src/changes.ts','packages/core/main/src/page-of.ts','packages/core/main/src/errors.ts','packages/core/main/src/documents.ts','packages/engine/core/src/mutation/Change.ts','packages/engine/core/src/mutation/ListPosition.ts','packages/engine/core/src/engine/DocumentPagesService.ts','packages/engine/core/src/events/DocumentEvent.ts','packages/engine/core/src/mutation/PageInsertResult.ts','packages/engine/core/src/dto/PageLayout.ts']
paths=[p for p in paths if (ROOT/p).exists()]
commits={k:subprocess.check_output(['git','rev-parse',v],cwd=ROOT,text=True).strip()for k,v in {'stagedEdits':'291be55e5','permissions':'5443a5649','objectNumber':'595bdc6c7','pageSpace':'6153c181e'}.items()}
for tag,ref in commits.items():
 for name in ['controller','contract','model']:
  p='packages/plugin/page-edit/src/'+name+'.ts';r=subprocess.run(['git','show',ref+'^:'+p],cwd=ROOT,capture_output=True)
  if r.returncode==0:(H/(tag+'-parent-'+name+'.ts.txt')).write_bytes(r.stdout)
write('source-evidence.json',{'base':BASE,'commits':commits,'currentOwners':[{'path':p,'sha256':sha((ROOT/p).read_bytes())}for p in paths],'rules':{'staging':'291be55e5 controller stages Change ops, labels and undo predictions; changes.ts sends immediately in staging order and engine owns order','positions':'291be55e5 move/placement index API becomes reorder/PagePosition neighbour or start/end; positionOf validates refs before sizeAt, even with explicit size','validation':'refsOf requires nonempty and pageOf existence; rotateBy type is quarter/half turns and PdfRotation required on every PageLayout','permissions':'controller refuse checks assemble/download then caller signal; duplicate also asserts download; other document extract is source engine authority','registry':'documents consumes full confirmed layout facts, events before promise settlement; returned result alone does not update registry','history':'rotationOf folds pending shows, rotate groups all ops into one change with undo groups; extraction sends holds and waits for changes.whenSettled','lifetime':'close cancels change queue with instance-closed; captured capability authority remains readable but writes cannot stage','boundaries':'Fake table supplies explicit typed engine facts/results only. No actual PDF parsing/editing claim; actual shipping controller/kernel/change queue execute.'}})
retired={
'rotate-by':'Old arbitrary deltas 45/450/-720/-900/0.5/NaN retire outside current rotateBy 90|-90|180 contract. Retain quarter turns, negative wrapping, mixed initial rotations and unknown refs; direct tests add pending turns.',
'passthroughs':'Old move/raw index, empty passthrough and arbitrary engine public answers retire; reorder validates refs and stages pages.reorder, void returns, and confirmed registry changes preserve edit intent.',
'refused-by-the-engine':'Old no-local-gate requirement retires: all local verbs assert permissions before staging. Separate permitted document still witnesses engine Forbidden mapping; extract/duplicate download authority added.',
'deferred-and-aborted':'Old await-prior-response dispatch retires: current queue sends apply calls immediately in staging order, engine owns serialization. Each deferred response still controls promise and confirmed layout; direct extraction barrier/cancel tests retain timing intent.',
'add-blank':'Old numeric/clamped/negative/out-of-range index placement and ambiguous index+after/after+before inputs retire under neighbour/start/end PagePosition. Retain append, empty options, before/after each anchor, explicit count3/2,size612x792/1x2, missing anchors through scenario plus six direct matrix rows.',
'add-blank-fractional-index':'Old fractional/NaN predecessor TypeErrors and explicit-size bypass retire; malformed numeric neighbour refs now reject exact not-found even with size. Original source preserved verbatim.',
'insert':'Raw placement index7 retires to neighbour/end positions. Source bytes, empty bytes, default and missing-anchor refusal remain; public result now pages from ChangeResult.',
'a-captured-capability-outlives-its-document':'Old post-close page service calls retire: change queue rejects the document closed before engine call. Captured canEdit remains true, unresolved token absent, ref-based rotate/insert reject instance-closed.',
'an-empty-registry':'Old missing ref rotation=0 and raw/fractional index placements retire. Current missing refs reject before staging; Letter default and insertion into empty registry remain.',
'rotate-by-groups-and-empty':'Old multiple independent service promises and last answer retire; all rotation groups now form one atomic apply/history entry and public rotation returns void. First-seen groups and empty refusal remain.',
'move-to-the-end':'move and omitted/raw index position retire to reorder with explicit end or validated neighbour positions. Order, end/ref placement and missing-anchor intent remain.',
'insert-a-subset-of-another-pdf':'Empty source indexes no longer extract/insert empty PDF: refsOf no-pages refusal before opening. Success [2,0] source order/full scope/id/extract/close, invalid source index close and open refusal remain.',
'rotate-by-a-page-without-a-rotation':'Undefined rotation is outside mandatory typed PageLayout and malformed NaN grouping retires; current valid registry mixed-rotation grouping independently asserted. Original malformed input preserved in history.',
'insert-from-another-document':'Old direct unvalidated refs retire; source registry resolves refs/indexes before extraction. Retain open source, default/explicit placement, absent source and refused source download; add missing source ref before extraction.',
'duplicate':'Old placement/raw service return retires; current options.position, assemble+download authority, pending-settle barrier and staged duplicate label preserve copy/extract intent. Unknown/empty refusals stay exact.',
'state-and-scope':'Empty document slice retires to selected refs state. Pre-open absence/authority remain; selection resolving/dedup/clear and deletion projection added as direct witnesses.'}
def differences(a,b,path='$'):
 if a==b:return []
 if type(a)!=type(b)or not isinstance(a,(dict,list)):return[path]
 if isinstance(a,list):return[p for i in range(max(len(a),len(b)))for p in(differences(a[i],b[i],f'{path}[{i}]')if i<len(a)and i<len(b)else[f'{path}[{i}]'])]
 return[p for k in dict.fromkeys([*a,*b])for p in(differences(a[k],b[k],path+'.'+k)if k in a and k in b else[path+'.'+k])]
mapping=[]
for i,o in enumerate(old):
 c=next(c for c in current if c['name']==o['name']);out=o.get('result',o.get('runnerException'))
 mapping.append({'originalOccurrence':i,'file':o['file'],'kind':o['kind'],'name':o['name'],'originalInput':o['input'],'originalInputSha256':o['inputSha256'],'originalOutcome':out,'originalOutcomeSha256':digest(out),'currentFile':c['file'],'currentKind':c['kind'],'currentIndex':c['index'],'currentInputSha256':c['inputSha256'],'currentOutcomeSha256':digest(c['result']),'changedInputPaths':differences(o['input'],c['input']),'partialRetirement':retired.get(o['name']),'sourceEvidence':'source-evidence.json','retainedIntent':c['input']['covers']})
write('accounting.json',{'summary':{'original':19,'current':19,'mapped':19,'wholeRetirements':0,'partialRetirements':len(retired),'additions':0,'changedInputs':19},'mappings':mapping,'additions':[]})
write('task11-current-drift.json',[x for x in json.loads((frozen/'task11-current-drift.json').read_text()) if x['source']=='page-edit'])
counts=collections.Counter();negative=[]
for c in current:
 r=c['result'];assert not r['unmatched'];counts['steps']+=len(r['trace'])
 for i,t in enumerate(r['trace']):
  counts['callbacks']+=len(t['callbacks']);counts['settlements']+=len(t['settled']);assert not t['reports']
  if 'threw'in t.get('outcome',{}):negative.append({'name':c['name'],'step':i,'channel':'outcome.threw','value':t['outcome']['threw']});counts['stepThrows']+=1
  for s in t['settled']:
   if 'rejected'in s:negative.append({'name':c['name'],'step':i,'channel':'settled.rejected','value':s['rejected']});counts['rejections']+=1
write('negative-channels.json',negative);write('runtime-summary.json',dict(counts,scenarios=len(current),runnerExceptions=0,unmatched=0,reports=0))
write('direct-test-contracts.json',{'file':'test/page-edit-current.test.ts','trackedAdditionalWitnesses':['Two pending turns: full before/after registry, undo predictions, rotate labels, callback-before-promise order','Duplicate extraction waits for prior changes; duplicate/blank/delete labels and counts','Six independent blank size/position/count/full confirmed layout matrix rows','Pre-aborted all nine verbs; permission precedes abort; duplicate separately needs download','After-stage cancellation rejects only caller; eventual confirmed edit applies; delete-all refusal','Close rejects pending edit/removes all subscriptions/suppresses late facts','Selection identity/dedup and deleted-page projection','Reorder and whole-PDF insert label scope; unconfirmed result does not mutate registry'],'negativeChannels':['operation-cancelled: pageEdit.<verb> was cancelled','operation-cancelled: operation cancelled','permission-denied: pageEdit.duplicate requires doc.download','permission-denied: pageEdit.extract requires doc.download','invalid-input: a document keeps at least one page','instance-closed: the document closed']})
if '--reconcile-only' in sys.argv:
 files=load('preservation.json')['files']
else:
 exempt={'tooling/plugin-conformance/src/suites/page-edit.ts','tooling/plugin-conformance/test/commands-current.test.ts'};files=[]
 for p in subprocess.check_output(['git','ls-tree','-r','--name-only',BASE,'tooling/plugin-conformance'],cwd=ROOT,text=True).splitlines():
  if p in exempt:continue
  before=blob(p);assert before==(ROOT/p).read_bytes(),p;files.append({'path':p,'sha256':sha(before)})
 write('preservation.json',{'base':BASE,'purpose':'One-time audit evidence only; never a future-source guard','exempt':sorted(exempt),'preservedCount':len(files),'files':files})
write('integrity.json',{p.name:sha(p.read_bytes())for p in H.iterdir()if p.suffix in ['.json','.txt']and p.name!='integrity.json'})
print(json.dumps({'accounting':load('accounting.json')['summary'],'runtime':load('runtime-summary.json'),'preserved':len(files)},indent=2))
