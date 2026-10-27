"""Bounded C9 accounting. A broad preservation pass runs once, never as a future suite guard."""
import pathlib,json,hashlib,subprocess,collections,sys,re
H=pathlib.Path(__file__).resolve().parent;ROOT=H.parents[3];BASE='ca63bf57a31ce722cefc8e03944390c4c7f5e745'
def sha(b):return hashlib.sha256(b).hexdigest()
def digest(v):return sha(json.dumps(v,ensure_ascii=False,separators=(',',':')).encode())
def load(p):return json.loads((H/p).read_text())
def write(p,v):(H/p).write_text(json.dumps(v,ensure_ascii=False,indent=2)+'\n')
frozen=ROOT/'.superpowers/sdd/multiplatform-upstream-catchup/task-12b2c-before'
old=load('original-runtime.json');cur=load('current-inputs-runtime.json')
assert len(old)==635 and len(cur)==642
assert old==[r for r in json.loads((frozen/'inputs-runtime.json').read_text())if r['source']=='i18n']
assert (H/'original-i18n.ts.txt').read_bytes()==(frozen/'i18n.ts.txt').read_bytes()
assert (H/'original-i18n.ts.txt').read_bytes()==subprocess.check_output(['git','show',BASE+':tooling/plugin-conformance/src/suites/i18n.ts'],cwd=ROOT)
assert (H/'original-commands-current.test.ts.txt').read_bytes()==subprocess.check_output(['git','show',BASE+':tooling/plugin-conformance/test/commands-current.test.ts'],cwd=ROOT)
for f in ['original-runtime.json','original-i18n.ts.txt','original-commands-current.test.ts.txt']:print(f,sha((H/f).read_bytes()))
def diff(a,b,path='$'):
 if a==b:return []
 if type(a)!=type(b)or not isinstance(a,(dict,list)):return[path]
 if isinstance(a,list):return[p for i in range(max(len(a),len(b)))for p in(diff(a[i],b[i],f'{path}[{i}]')if i<len(a)and i<len(b)else[f'{path}[{i}]'])]
 return[p for k in dict.fromkeys([*a,*b])for p in(diff(a[k],b[k],path+'.'+k)if k in a and k in b else[path+'.'+k])]
seen=collections.Counter();mapped=[];used=set()
partial={
 'initial-defaults':'Configuration initialization now explicitly seeds empty state via seedFromSettings; fallbackLocale remains settings-only, waitingTranslations is mandatory.',
 'add-translations-unknown-is-ignored':'Old omitted waitingTranslations caused a TypeError; current mandatory state queues unknown-code strings. Capability still refuses a code with no loader (separate live witness).',
 'lazy-startup-locale-announces-nothing':'Current state listener announces the loaded startup language against its fallback; silence while still loading remains.',
 'destroy-releases-the-watch':'Kernel destruction removes watch; late load writes report exact instance-closed and are dropped. Late loader fulfillment resolves its waiter after the closed-state write is dropped; closed state remains unchanged.',
 'loader-throws-at-startup':'A synchronous loader throw fails kernel start (not a rejected Promise load-failure event). Subsequent token access fails kernel scope.',
 'loader-throws-in-the-watch':'A synchronous loader throw from watch reports directly; it is not transformed into asynchronous locale-load failure.',
 'translate-fallback-locale':'Current bundled strings fill Spanish zoom.in before English fallback pack; expected Acercar.',
}
for i,o in enumerate(old):
 key=(o['file'],o['name']);occ=seen[key];seen[key]+=1
 name=o['name']
 if name=='unknown-action-is-ignored':name='initial-empty-state'
 elif occ:name+=' explicit-tag occurrence'
 matches=[(j,c)for j,c in enumerate(cur)if c['file']==o['file']and c['name']==name];assert len(matches)==1,(i,key,name)
 j,c=matches[0];assert j not in used;used.add(j)
 retire=partial.get(o['name'])
 if o['input'].get('fn')=='i18nReducer':retire=retire or 'Removed action-envelope reducer replaced by actual named shipping transition; legacy same/next wrapper and settings-owned fallbackLocale leave the replay schema. Identity is asserted directly.'
 if o['input'].get('fn')=='initialI18nState':retire=retire or 'Old config-taking initialI18nState replaced by empty initialI18nState then seedFromSettings with full current settings-shaped arguments. Pure seed accepts unknown loader markers; no marker is invoked as a loader.'
 whole=o['name']=='unknown-action-is-ignored'
 mapped.append({'originalOccurrence':i,'originalFileIndex':sum(x['file']==o['file']for x in old[:i]),'file':o['file'],'originalName':o['name'],'duplicateOccurrence':occ,'originalInput':o['input'],'originalInputSha256':o['inputSha256'],'originalOutcome':o.get('outcome',o.get('result',o.get('runnerException'))),'currentFile':c['file'],'currentIndex':c['index'],'currentName':name,'currentInput':c['input'],'currentInputSha256':c['inputSha256'],'currentOutcome':c.get('outcome',c.get('result')),'currentOutcomeSha256':digest(c.get('outcome',c.get('result'))),'changedInputPaths':diff(o['input'],c['input']),'disposition':'whole-retired-with-replacement'if whole else'partial-retirement'if retire else'preserved-current-contract','retirement':('Unknown action dispatch has no entry point in current named transitions. Original full input/outcome remains immutable; empty state is an explicit replacement, not same semantics.'if whole else retire),'sourceEvidence':'source-evidence.json'})
add=[{'currentFile':c['file'],'currentIndex':c['index'],'name':c['name'],'currentInput':c['input'],'currentInputSha256':c['inputSha256'],'currentOutcome':c.get('outcome',c.get('result')),'reason':'Current fallbackKey, bundled precedence, dictionary merge/nesting or waiting-pack contract coverage'}for j,c in enumerate(cur)if j not in used]
summary={'original':635,'originalPure':611,'originalLive':24,'current':642,'currentPure':618,'currentLive':24,'mapped':len(mapped),'pluralRows':532,'duplicateExtras':4,'wholeRetirements':1,'survivingOriginals':634,'replacementRows':1,'totalNewRows':8,'partialRetirements':sum(m['disposition']=='partial-retirement'for m in mapped),'additions':len(add),'changedInputs':sum(bool(m['changedInputPaths'])for m in mapped)}
write('accounting.json',{'summary':summary,'arithmetic':'635 originals - 1 whole retirement + 1 mapped replacement + 7 additional rows = 642 current','mappings':mapped,'replacements':[{'originalOccurrence':m['originalOccurrence'],'currentFile':m['currentFile'],'currentIndex':m['currentIndex'],'currentName':m['currentName'],'currentInput':m['currentInput'],'currentOutcome':m['currentOutcome'],'reason':m['retirement']}for m in mapped if m['disposition']=='whole-retired-with-replacement'],'additions':add})
paths=['packages/plugin/i18n/src/'+p for p in ['controller.ts','contract.ts','model.ts','translate.ts','negotiate.ts','sync/loaders.ts','strings.ts','i18n.plugin.ts']];paths=[p for p in paths if(ROOT/p).exists()]
commits={k:subprocess.check_output(['git','rev-parse',v],cwd=ROOT,text=True).strip()for k,v in {'namedTransitions':'4fc1a32af','settingsContract':'cdf4586c7','translationFallbackKeyAndBundles':'3ea7cb1f3','scriptAwareNegotiation':'277961ad3'}.items()}
for label,ref in commits.items():
 for part in ['model','contract','controller','translate']:
  p='packages/plugin/i18n/src/'+part+'.ts';r=subprocess.run(['git','show',ref+'^:'+p],cwd=ROOT,capture_output=True)
  if r.returncode==0:(H/(label+'-parent-'+part+'.ts.txt')).write_bytes(r.stdout)
write('source-evidence.json',{'base':BASE,'commits':commits,'currentOwners':[{'path':p,'sha256':sha((ROOT/p).read_bytes())}for p in paths],'contracts':{'state':'model.ts: initial empty locale/locales/loading/waitingTranslations; seedFromSettings, named transitions, no reducer','translation':'translate.ts: TranslationSources carries fallbackLocale separately, app then bundled current language then fallback language; fallbackKey tried inside each language','settings':'controller.ts: immutable settings read, follows locales and locale updates; loaders built before plugin registration','direction':'Locale.direction, default ltr; no old rtl boolean','loaders':'sync/loaders.ts: watch loading identity dedups same code; late other-code pack registers without switching; synchronous throws report/start failure, rejected loads emit exact PluginErrorInfo','disposal':'after teardown watch removed and closed state rejects writes; no fake late completion of captured waiter','boundary':'explicit current inputs are the authority, no copied translation/plural rules; scripted JSON loader decoding is private transport only'}})
write('task11-current-drift.json',[x for x in json.loads((frozen/'task11-current-drift.json').read_text())if x['source']=='i18n'])
negative=[];counts=collections.Counter()
for c in cur:
 if c['kind']=='function':
  assert 'returned'in c['outcome'];continue
 r=c['result'];assert not r['unmatched'];counts['scenarios']+=1
 for i,t in enumerate(r['trace']):
  counts['steps']+=1;counts['callbacks']+=len(t['callbacks']);counts['settlements']+=len(t['settled'])
  if 'threw'in t.get('outcome',{}):negative.append({'name':c['name'],'step':i,'channel':'outcome.threw','value':t['outcome']['threw']});counts['stepThrows']+=1
  for s in t['settled']:
   if'rejected'in s:negative.append({'name':c['name'],'step':i,'channel':'settled.rejected','promise':s['promise'],'value':s['rejected']});counts['rejections']+=1
  for p in t['reports']:negative.append({'name':c['name'],'step':i,'channel':'reports','value':p});counts['reports']+=1
  for call in t['callbacks']:
   if call['fn']in ['failed','console.warn']:negative.append({'name':c['name'],'step':i,'channel':'callback.'+call['fn'],'value':call});counts['failureCallbacks'if call['fn']=='failed'else'warnings']+=1
write('negative-channels.json',negative);write('runtime-summary.json',dict(counts,pure=618,pluralRows=532,unmatched=0,runnerExceptions=0,pureThrows=0))
if '--reconcile-only'not in sys.argv:
 exempt={'tooling/plugin-conformance/src/suites/i18n.ts','tooling/plugin-conformance/test/commands-current.test.ts'};files=[]
 for p in subprocess.check_output(['git','ls-tree','-r','--name-only',BASE,'tooling/plugin-conformance'],cwd=ROOT,text=True).splitlines():
  if p in exempt:continue
  b=subprocess.check_output(['git','show',BASE+':'+p],cwd=ROOT);assert b==(ROOT/p).read_bytes(),p;files.append({'path':p,'sha256':sha(b)})
 write('preservation.json',{'base':BASE,'purpose':'One-time audit evidence only; never future suite byte guards','exempt':sorted(exempt),'preservedCount':len(files),'files':files})
log=(H/'full-typecheck.out').read_text();allocation=collections.Counter(re.findall(r'^(.+?)\(\d+,\d+\): error TS',log,re.M));assert not any('i18n'in p for p in allocation),allocation
write('diagnostics.json',{'total':sum(allocation.values()),'allocation':dict(allocation),'owned':0})
write('direct-test-contracts.json',{'witnesses':['Immutable settings add/remove/reset locales, fallback-only invalidation, actual replaced loader called once; translator/list reference stability','Cancellation exact AbortError cause, late pack registration without switch/event, pre-aborted refusal before loader'],'source':'test/i18n-current.test.ts','vectorAdditions':False})
write('integrity.json',{p.name:sha(p.read_bytes())for p in H.iterdir()if p.suffix in ['.json','.txt']and p.name!='integrity.json'})
print(json.dumps({'accounting':summary,'runtime':load('runtime-summary.json'),'diagnostics':load('diagnostics.json'),'preserved':load('preservation.json')['preservedCount']},indent=2))
