"""C10 occurrence accounting and one-time BASE preservation; no active future-suite guard."""
import json,pathlib,hashlib,subprocess,collections,re,sys
H=pathlib.Path(__file__).resolve().parent;ROOT=H.parents[3];BASE='fcf0350e067e257b46a2b7d7e826886bd5ff0031'
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
assert len(old)==38 and len(cur)==47
assert old==[r for r in json.loads((frozen/'inputs-runtime.json').read_text())if r['source']=='shell']
for archive,path in [('original-shell.ts.txt','tooling/plugin-conformance/src/suites/shell.ts'),('original-commands-current.test.ts.txt','tooling/plugin-conformance/test/commands-current.test.ts')]:
 assert (H/archive).read_bytes()==subprocess.check_output(['git','show',BASE+':'+path],cwd=ROOT)
assert(H/'original-shell.ts.txt').read_bytes()==(frozen/'shell.ts.txt').read_bytes()
mappings=[];used=set()
for i,o in enumerate(old):
 name='initial-empty-state'if o['name']=='unknown-action-is-ignored'else o['name'];matches=[(j,c)for j,c in enumerate(cur)if c['file']==o['file']and c['name']==name];assert len(matches)==1
 j,c=matches[0];assert j not in used;used.add(j)
 whole=o['name']=='unknown-action-is-ignored';partial=o['input'].get('fn')=='shellReducer'and not whole
 reason='Removed arbitrary action dispatcher has no current shipping entry point; full original is archived. New empty initial state is a replacement, not semantic equivalence.'if whole else'Retired local action envelope and same/next adapter; replay current named transition with actual positional args/current mandatory focusRequest fields. Assert identity directly.'if partial else None
 mappings.append({'originalOccurrence':i,'originalFileIndex':sum(r['file']==o['file']for r in old[:i]),'originalName':o['name'],'originalFile':o['file'],'originalInput':o['input'],'originalInputSha256':o['inputSha256'],'originalOutcome':o.get('outcome',o.get('result',o.get('runnerException'))),'currentFile':c['file'],'currentIndex':c['index'],'currentName':c['name'],'currentInput':c['input'],'currentInputSha256':c['inputSha256'],'currentOutcome':c.get('outcome',c.get('result')),'currentOutcomeSha256':digest(c.get('outcome',c.get('result'))),'changedInputPaths':diff(o['input'],c['input']),'disposition':'whole-retired-with-replacement'if whole else'partial-retirement'if partial else'preserved-current-contract','reason':reason,'sourceEvidence':'source-evidence.json'})
additions=[dict(c,reason='New current focus moment, snapshot projection or toggleMenu shipping contract.')for j,c in enumerate(cur)if j not in used]
summary={'original':38,'originalPure':32,'originalLive':6,'current':47,'currentPure':40,'currentLive':7,'mapped':len(mappings),'wholeRetirements':1,'survivingOriginals':37,'replacementRows':1,'totalNewRows':10,'partialRetirements':sum(m['disposition']=='partial-retirement'for m in mappings),'additions':len(additions),'changedInputs':sum(bool(m['changedInputPaths'])for m in mappings)}
write('accounting.json',{'summary':summary,'arithmetic':'38 originals - 1 whole retirement + 1 mapped replacement + 9 additional rows =47 current','mappings':mappings,'replacements':[m for m in mappings if m['disposition']=='whole-retired-with-replacement'],'additions':additions})
commits={label:subprocess.check_output(['git','rev-parse',ref],cwd=ROOT,text=True).strip()for label,ref in {'namedTransitions':'4fc1a32af','focusMoments':'d6dd4f283'}.items()}
paths=['packages/plugin/shell/src/'+p for p in ['model.ts','controller.ts','contract.ts','shell.plugin.ts','state.ts','token.ts','index.ts']]+['packages/plugin/shell/test/shell.test.ts']
write('source-evidence.json',{'base':BASE,'commits':commits,'owners':[{'path':p,'sha256':sha((ROOT/p).read_bytes())}for p in paths],'contracts':{'model':'mandatory focusRequest; open increments only on focus=true, retains count on unfocused open; close/exclusive/toggle/closeAll clear to0; snapshotOf omits counts; applySnapshot resets to0','construction':'shellPlugin defines document scope, token, state initialShellState and createShellController; current existing factory already constructs it','events':'controller state change diff; opens have id/props, closes id only; menu closes before opens; repeated focus and prop changes produce no opening event','prototype':'Current plain object access observes inherited toString/valueOf. Preserve named existing adversarial input and exact current output; do not normalize or copy shell rules locally.','identity':'Ignored model operations return identical state; memoByKey preserves unrelated public record; repeated focused open produces new record/list; snapshot memoized until state changes','retirement':'Original action-envelope adapter is private old lens, not a shipping parity target. Full inputs and outcomes archived with replacement explicitly distinguished.'}})
drift=[r for r in json.loads((frozen/'task11-current-drift.json').read_text())if r['source']=='shell'];assert not drift;write('task11-current-drift.json',drift)
counts=collections.Counter();negative=[]
for c in cur:
 if c['kind']=='function':assert'returned'in c['outcome'];continue
 r=c['result'];assert not r['unmatched'];counts['scenarios']+=1
 for i,t in enumerate(r['trace']):
  counts['steps']+=1;counts['callbacks']+=len(t['callbacks']);counts['settlements']+=len(t['settled'])
  if'threw'in t.get('outcome',{}):negative.append({'name':c['name'],'step':i,'channel':'outcome.threw','value':t['outcome']['threw']});counts['stepThrows']+=1
  assert not t['reports'];assert not any('rejected'in s for s in t['settled'])
write('negative-channels.json',negative);write('runtime-summary.json',dict(counts,pure=40,pureThrows=0,runnerExceptions=0,unmatched=0,rejections=0,reports=0))
if '--reconcile-only'not in sys.argv:
 exempt={'tooling/plugin-conformance/src/suites/shell.ts','tooling/plugin-conformance/test/commands-current.test.ts'};files=[]
 for p in subprocess.check_output(['git','ls-tree','-r','--name-only',BASE,'tooling/plugin-conformance'],cwd=ROOT,text=True).splitlines():
  if p in exempt:continue
  b=subprocess.check_output(['git','show',BASE+':'+p],cwd=ROOT);assert b==(ROOT/p).read_bytes(),p;files.append({'path':p,'sha256':sha(b)})
 write('preservation.json',{'base':BASE,'purpose':'one-time audit only; never future suite guards','exempt':sorted(exempt),'preservedCount':len(files),'files':files})
allocation=collections.Counter(re.findall(r'^(.+?)\(\d+,\d+\): error TS',(H/'full-typecheck.out').read_text(),re.M));assert not any('shell'in p for p in allocation)
write('diagnostics.json',{'total':sum(allocation.values()),'allocation':dict(allocation),'owned':0})
write('integrity.json',{p.name:sha(p.read_bytes())for p in H.iterdir()if p.suffix in ['.json','.txt']and p.name!='integrity.json'})
print(json.dumps({'summary':summary,'runtime':load('runtime-summary.json'),'diagnostics':load('diagnostics.json'),'preserved':load('preservation.json')['preservedCount']},indent=2))
