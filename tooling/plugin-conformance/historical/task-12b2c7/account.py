"""One-time original/current metadata reconciliation and bounded BASE preservation audit."""
import pathlib,json,hashlib,subprocess,collections
ROOT=pathlib.Path(__file__).resolve().parents[4]; H=pathlib.Path(__file__).resolve().parent
BASE='7a9d07f4da2414d743edba67c66eb9fd1919e7fe'
def sha(v):return hashlib.sha256(v).hexdigest()
def digest(v):return sha(json.dumps(v,separators=(',',':'),ensure_ascii=False).encode())
def write(p,v):(H/p).write_text(json.dumps(v,indent=2,ensure_ascii=False)+'\n')
def load(p):return json.loads((H/p).read_text())
def blob(p,ref=BASE):return subprocess.check_output(['git','show',ref+':'+p],cwd=ROOT)
old=load('original-runtime.json');current=load('current-inputs-runtime.json');assert len(old)==14 and len(current)==17
frozen=ROOT/'.superpowers/sdd/multiplatform-upstream-catchup/task-12b2c-before'
assert (H/'original-metadata.ts.txt').read_bytes()==(frozen/'metadata.ts.txt').read_bytes()
assert old==[r for r in json.loads((frozen/'inputs-runtime.json').read_text()) if r['source']=='metadata']
paths=['packages/plugin/metadata/src/controller.ts','packages/plugin/metadata/src/contract.ts','packages/plugin/metadata/src/model.ts','packages/engine/core/src/dto/DocumentMetadata.ts','packages/engine/core/src/dto/MetadataPatch.ts','packages/engine/core/src/engine/MetadataService.ts','packages/engine/core/src/engine/CustomMetadataService.ts','packages/engine/core/src/events/DocumentEvent.ts','packages/core/main/src/mirror.ts','packages/core/main/src/errors.ts','packages/engine/core/src/auth/scope/errors.ts']
commits={'onDemand':'3ea7cb1f3e5b11e1e75f1f4649fae4f07004e3fa','customSplit':'a4b8259f74637c2d096fbb4d88fa5009dcb82ebf','mirrorOwnership':'4fc1a32af6d6ece9022a08139ac721e5c6fa8411','capabilityVocabulary':'cdf4586c7115f64d7ec80f362149c6b5bc6cc719'}
for tag,ref in commits.items():
 for name in ['controller','contract','model']:
  p='packages/plugin/metadata/src/'+name+'.ts'
  result=subprocess.run(['git','show',ref+'^:'+p],cwd=ROOT,capture_output=True)
  if result.returncode==0:(H/(tag+'-parent-'+name+'.ts.txt')).write_bytes(result.stdout)
write('source-evidence.json',{'base':BASE,'commits':commits,'currentOwners':[{'path':p,'sha256':sha((ROOT/p).read_bytes())}for p in paths], 'rules':{'lazy':'controller.ts onDemand:true / ensureLoaded; 3ea7cb1f3 adds explicit load to both mirrors','split':'a4b8259f7 splits custom keys and replaces JSON custom comparison with shallow union of keys','loads':'controller changed cause=load emits onResynced only; confirmed events emit onUpdated','refresh':'core mirror.ts refresh sets loadAgain when running; ensureLoaded returns running without requesting re-read','errors':'toPluginError maps Forbidden code and details.required; class name alone is not a permission error','lifetime':'guarded service and ctx.cancellable reject writes on close; mirror suppresses instance-close load failure','dto':'DocumentMetadata dates are createdAt/modifiedAt; standard/custom results include full MutationMeta, public API returns just metadata/custom'}})
retired={
'seeds-from-a-read':'Eager connect-time metadata.read is removed by onDemand; preserve null/in-flight/loaded values via explicit ensureLoaded. Combined custom keys load independently through custom.get.',
'a-failed-seed-read-is-ignored':'Background eager read with swallowed rejection retires: explicit ensureLoaded rejects its caller; no reports, null/error metadata, independent ready custom mirror survive.',
'update-writes-through-and-sets':'Public result drops old cache:null and returns {metadata}; full MutationMeta stays at engine boundary. Confirmed-event-before-resolution and exact three-state patch survive.',
'reload':'Automatic first read is replaced by explicit ensureLoaded. Failed refresh now retains last confirmed data with error status; standard refresh does not reload custom.',
'a-forbidden-seed-read':'Eager first read retires; explicit ensureLoaded rejects. PermissionDenied now carries real Forbidden/details.required, preserving forbidden/error/ready sequence.',
'status-while-the-seed-read-is-in-flight':'Eager loading retires; open is idle. ensureLoaded joins both in-flight reads; current explicit refresh queues one standard re-read rather than deduplicating that request.',
'an-event-during-the-seed-read-wins':'Eager connect read retires; same event-wins race begins via ensureLoaded and now covers both standard/custom mirrors.',
'closing-during-the-seed-read':'Eager connect read retires; explicit load is ended at close, with mirror instance-close suppression and no late load callbacks. Add reopen idle witnesses.',
'on-updated-reports-each-confirmed-change':'Loads no longer emit updated/system origin; onResynced carries load values separately. Custom JSON-order comparisons retire to independent shallow custom keys; malformed sparse standard input is completed to current DTO.',
'changed-keys-beyond-the-info-dict':'Arbitrary number/object/array fields are outside current DocumentMetadata/CustomMetadata DTO. Preserve equal/different-key intent as typed custom strings; object identity and JSON custom-order behavior retire under a4b8259f7 model/DTO evidence.'}
def differences(a,b,path='$'):
 if a==b:return []
 if type(a)!=type(b) or not isinstance(a,(dict,list)):return [path]
 if isinstance(a,list):return [p for i in range(max(len(a),len(b))) for p in (differences(a[i],b[i],f'{path}[{i}]')if i<len(a)and i<len(b)else[f'{path}[{i}]'])]
 return [p for k in dict.fromkeys([*a,*b]) for p in (differences(a[k],b[k],path+'.'+k)if k in a and k in b else[path+'.'+k])]
mappings=[]
for i,o in enumerate(old):
 c=next(c for c in current if c['name']==o['name']);outcome=o.get('result',o.get('runnerException'))
 mappings.append({'originalOccurrence':i,'file':o['file'],'kind':o['kind'],'name':o['name'],'originalInput':o['input'],'originalInputSha256':o['inputSha256'],'originalOutcome':outcome,'originalOutcomeSha256':digest(outcome),'currentFile':c['file'],'currentKind':c['kind'],'currentIndex':c['index'],'currentInputSha256':c['inputSha256'],'currentOutcomeSha256':digest(c['result']),'changedInputPaths':differences(o['input'],c['input']),'partialRetirement':retired.get(o['name']),'sourceEvidence':'source-evidence.json','retainedIntent':c['input']['covers']})
write('accounting.json',{'summary':{'original':14,'current':17,'mapped':14,'wholeRetirements':0,'partialRetirements':len(retired),'additions':3,'changedInputs':14},'mappings':mappings,'additions':[{'file':c['file'],'name':c['name'],'inputSha256':c['inputSha256'],'purpose':c['input']['covers']}for c in current if c['name']not in {o['name']for o in old}]})
write('task11-current-drift.json',[x for x in json.loads((frozen/'task11-current-drift.json').read_text()) if x['source']=='metadata'])
negative=[];counts=collections.Counter()
for c in current:
 r=c['result'];assert not r['unmatched']; counts['steps']+=len(r['trace'])
 for i,t in enumerate(r['trace']):
  assert 'threw'not in t.get('outcome',{});assert not t['reports'];counts['callbacks']+=len(t['callbacks']);counts['settlements']+=len(t['settled'])
  for s in t['settled']:
   if 'rejected'in s:negative.append({'name':c['name'],'step':i,'channel':'settled.rejected','value':s['rejected']})
write('negative-channels.json',negative);write('runtime-summary.json',dict(counts,scenarios=len(current),runnerExceptions=0,stepThrows=0,unmatched=0,reports=0,rejections=len(negative)))
write('direct-test-contracts.json',{'file':'test/metadata-current.test.ts','trackedAdditionalWitnesses':['Callbacks see new snapshots before immediate/deferred write resolution','Pre-aborted standard/custom writes make no engine calls; permission precedence','One cancelled load waiter leaves shared standard/custom reads running','Caller cancellation rejects both deferred writes without projecting results','Close rejects in-flight write, removes all subscribers and suppresses late confirmations'],'negativeChannels':['operation-cancelled: metadata.update was cancelled','operation-cancelled: metadata.custom.update was cancelled','operation-cancelled: operation cancelled','permission-denied: metadata.update requires doc.metadata.modify','permission-denied: metadata.custom.update requires doc.metadata.modify','instance-closed: document instance a#<instance> was closed']})
exempt={'tooling/plugin-conformance/src/suites/metadata.ts','tooling/plugin-conformance/test/commands-current.test.ts'}
files=[]
for p in subprocess.check_output(['git','ls-tree','-r','--name-only',BASE,'tooling/plugin-conformance'],cwd=ROOT,text=True).splitlines():
 if p in exempt:continue
 before=blob(p);after=(ROOT/p).read_bytes();assert before==after,p;files.append({'path':p,'sha256':sha(before)})
write('preservation.json',{'base':BASE,'purpose':'One-time audit evidence only; no permanent active-source hash guard','exempt':sorted(exempt),'preservedCount':len(files),'files':files})
write('integrity.json',{p.name:sha(p.read_bytes())for p in H.iterdir()if p.suffix in ['.json','.txt']and p.name!='integrity.json'})
print(json.dumps({'accounting':load('accounting.json')['summary'],'runtime':load('runtime-summary.json'),'preserved':len(files)},indent=2))
