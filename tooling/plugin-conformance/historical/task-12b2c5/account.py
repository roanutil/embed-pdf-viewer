"""Build current C5 accounting; immutable originals are never rewritten."""
import hashlib,json,pathlib,subprocess,collections
here=pathlib.Path(__file__).resolve().parent
repo=here.parents[3]
BASE='417838c1835da14c5f94164cbe3665bd0f35d29c'
def read(n):return json.loads((here/n).read_text())
def write(n,v):(here/n).write_text(json.dumps(v,indent=2,ensure_ascii=False)+'\n')
def digest(v):return hashlib.sha256((v if isinstance(v,bytes) else json.dumps(v,separators=(',',':'),ensure_ascii=False).encode())).hexdigest()
def git(*args):return subprocess.check_output(['git',*args],cwd=repo)
original=read('original-inputs.json');current=read('current-inputs-runtime.json')
assert len(original)==76 and len(current)==79
frozen=repo/'.superpowers/sdd/multiplatform-upstream-catchup/task-12b2c-before'
assert original==[x for x in json.loads((frozen/'original-inputs.json').read_text())if x['source']=='commands']
assert (here/'original-commands.ts.txt').read_bytes()==(frozen/'commands.ts.txt').read_bytes()
assert read('original-inputs-runtime.json')==[x for x in json.loads((frozen/'inputs-runtime.json').read_text())if x['source']=='commands']
def diff(a,b,path='$'):
 if a==b:return []
 if type(a)!=type(b) or not isinstance(a,(dict,list)):return [{'path':path,'before':a,'after':b}]
 if isinstance(a,list):
  return [z for i in range(max(len(a),len(b))) for z in (diff(a[i],b[i],path+f'[{i}]')if i<len(a)and i<len(b)else [{'path':path+f'[{i}]',**({'before':a[i]}if i<len(a)else{}),**({'after':b[i]}if i<len(b)else{})}])]
 return [z for k in dict.fromkeys([*a,*b]) for z in (diff(a[k],b[k],path+'.'+k)if k in a and k in b else [{'path':path+'.'+k,**({'before':a[k]}if k in a else{}),**({'after':b[k]}if k in b else{})}])]
settingsNames={x['name']for x in original if x['input'].get('fn')in ['commandsReducer','initialCommandsState']}
partial={n:'The retired state/reducer and action discriminator are replaced by current settings read/mutation. same now compares disabled-category list identity; equal replacement is a settings no-op. Unknown-action dispatch has no current equivalent; that sub-contract is retired and the surviving unchanged-list read is exercised.' for n in settingsNames}
partial.update({n:'Plugin state construction is retired; the current definition declares settings with defaults/registration and no state. Duplicate/shortcut validation remains lazy at the configured registry read, not the plugin factory.' for n in ['plugin-no-config','plugin-empty-config','plugin-disabled-categories','plugin-commands','plugin-duplicate-command-is-not-checked-here','plugin-bad-shortcut-is-not-checked-here']})
partial.update({
'command-ctx':'Retire core() and lazy capabilityThrows: current CommandContext has documentId/get/tryGet/args/signal, and capabilities are created transactionally before document publication. Use actual DocumentsToken.list for document order, current available capability and explicit missing/no-document/loading refusals. Add bound-scope success and explicit target override steps. Opaque null context argument is replaced by omission in derivations.',
'after-destroy':'Old member-order refs are replaced with current actual returned function identities. Definition-only reads, registrations and removers survive; active-document lookups now refuse through kernel guardUsable, and disposed settings updates do nothing without reports.',
'a-bad-config-refuses-the-kernel':'Configured duplicates no longer refuse construction: configuredCommands uses Map.set and the later definition wins. Preserve original name as historical key and assert current configured registration success, with independent duplicate runtime conflicts elsewhere.',
'a-bad-config-shortcut-refuses-the-kernel':'Shortcut validation is lazy at configured-registry access; the first listCommandIds step throws the actual parser error instead of a kernel construction refusal.',
'categories':'No old reducer dispatch/state-change stream exists. Settings are the live owner; turned-off definitions short-circuit enabled/visible derivations, and idempotent setting changes emit nothing.',
'register-many-shortcuts-and-can-execute':'A runtime replacement shadows configured commands. Disposing the replacement reveals the configured definition rather than deleting it. Old replacer cannot remove a newer owner.',
'icon-accent':'Empty accent object now resolves as absent since both channels are undefined; preserve literal success for both/individual colors and no accent for empty/null/undefined/throw.',
'i18n-labels':'Current i18n missing-key fallback is the command id, not labelKey, while explicit labelKey translation and fallback-language translation survive.',
})
mappings=[];counts=collections.Counter()
for occurrence,o in enumerate(original):
 assert digest(o['input'])==o['inputSha256']
 matches=[x for x in current if (x['file'],x['name'])==(o['file'],o['name'])];assert len(matches)==1,o['name'];n=matches[0]
 assert digest(n['input'])==n['inputSha256']
 changes=diff(o['input'],n['input']);counts['changedInputs']+=bool(changes)
 mappings.append({'originalOccurrence':occurrence,'originalSuiteIndex':sum(x['file']==o['file']for x in original[:occurrence]),'file':o['file'],'name':o['name'],'originalInputSha256':o['inputSha256'],'currentIndex':n['index'],'currentInputSha256':n['inputSha256'],'disposition':'current-shipping-replacement'if changes or o['name']in partial else 'unchanged-input-current-shipping-owner','inputChanges':changes,'partialRetirement':partial.get(o['name']),'sourceEvidence':['packages/plugin/commands/src/contract.ts','packages/plugin/commands/src/commands.plugin.ts','packages/plugin/commands/src/controller.ts','packages/plugin/commands/src/registry.ts','packages/plugin/commands/src/write/turning-off.ts','packages/plugin/commands/src/services/availability.ts']})
additions=[{'file':x['file'],'index':x['index'],'name':x['name'],'inputSha256':x['inputSha256'],'reason':'Current disabledReason equality contract'if x['kind']=='function'else'Current settings events, permission hide/disable distinction, shortcut refusal, per-command disablement and category boundary/reset semantics.'}for x in current if not any((x['file'],x['name'])==(o['file'],o['name'])for o in original)]
write('accounting.json',{'base':BASE,'frozenCAuthority':'79b16091b9950935e5df77966f4207a1eddcce07/task-12b2c-before','summary':{'original':76,'current':79,'mapped':len(mappings),'wholeRetirements':0,'partialRetirements':sum(x['partialRetirement']is not None for x in mappings),'additions':len(additions),**counts},'mappings':mappings,'additions':additions,'task11':{'originalFunctionCases':52,'originalScenarioCases':24,'inputDiff':0,'lost':0,'added':0,'source':'task-11-cases.out:41-42','disposition':'No Task11 commands input drift; current migrations and additions are exhaustively recorded here.'},'inheritedLaterDrift':read('task11-current-drift.json')})
# All tracked prior private-tooling artifacts are preserved against the dispatch base.
paths=git('ls-tree','-r','--name-only',BASE,'tooling/plugin-conformance').decode().splitlines()
preserved=[]
for p in paths:
 if p=='tooling/plugin-conformance/src/suites/commands.ts':continue
 before=git('show',BASE+':'+p);now=(repo/p).read_bytes();assert before==now,p
 preserved.append({'path':p,'sha256':digest(before)})
write('preservation.json',{'base':BASE,'files':preserved,'preservedCount':len(preserved)})
# Pin exact source provenance for every shipping owner, including lifecycle and settings.
paths=['packages/plugin/commands/src/'+p for p in ['contract.ts','host-contract.ts','commands.plugin.ts','controller.ts','registry.ts','services/availability.ts','write/turning-off.ts']]+['packages/core/main/src/'+p for p in ['kernel.ts','settings.ts','testing.ts','event-hook.ts']]+['packages/core/ui/src/shortcuts.ts','packages/plugin/i18n/src/controller.ts','packages/plugin/i18n/src/translate.ts','packages/plugin/shell/src/controller.ts','packages/engine/core/src/dto/PageLayout.ts','packages/engine/core/src/identity/PageRef.ts']
paths=[p for p in paths if (repo/p).exists()]
write('source-evidence.json',{'base':BASE,'currentOwners':[{'path':p,'sha256':digest((repo/p).read_bytes()),'lastOwnerCommit':git('log','-1','--format=%H',BASE,'--',p).decode().strip(),'definition':(repo/p).read_text()}for p in paths],'removals':[{'commit':c,'path':p,'definition':git('show',c+':'+p).decode()}for c,p in [('4fc1a32af^','packages/plugin/commands/src/model.ts'),('4fc1a32af^','packages/plugin/commands/src/registry.ts'),('cdf4586c7^','packages/plugin/commands/src/model.ts')]],'removalDiffs':{c:git('show',c,'--','packages/plugin/commands/src').decode()for c in ['4fc1a32af','cdf4586c7']}})
# Enumerate every error payload, returned refusal and warning with an explicit source owner.
def diagnostics(v,path='$'):
 if isinstance(v,dict):
  if '$error'in v:yield {'path':path,'kind':'error','value':v}
  if (v.get('status')=='rejected' or v.get('reason')in ['turned-off','not-allowed'])and 'reason'in v:yield {'path':path,'kind':'returned-refusal','value':v}
  if isinstance(v.get('fn'),str)and v['fn'].startswith('console.'):yield {'path':path,'kind':'console','value':v}
  for k,x in v.items():yield from diagnostics(x,path+'.'+k)
 elif isinstance(v,list):
  for i,x in enumerate(v):yield from diagnostics(x,path+f'[{i}]')
negative=[]
for x in current:
 for d in diagnostics(x.get('result',x.get('outcome'))):
  name=x['name'];owner='packages/plugin/commands/src/controller.ts'
  if name=='after-destroy' or name=='command-ctx':owner='packages/core/main/src/kernel.ts'
  elif '.reports[' in d['path']:owner='packages/core/main/src/event-hook.ts'
  elif 'modifier'in json.dumps(d['value']):owner='packages/core/ui/src/shortcuts.ts'
  elif d['kind']=='console':owner='packages/plugin/i18n/src/controller.ts'
  elif d['kind']=='returned-refusal':owner='packages/plugin/commands/src/services/availability.ts'if d['value'].get('reason')in ['turned-off','not-allowed']else'packages/plugin/commands/src/controller.ts'
  negative.append({'file':x['file'],'name':name,**d,'sourceOwner':owner})
write('named-diagnostics.json',negative)
ts=[t for x in current for t in x.get('result',{}).get('trace',[])]
write('runtime-summary.json',{'cases':len(current),'pure':sum(x['kind']=='function'for x in current),'scenarios':sum(x['kind']=='scenario'for x in current),'steps':len(ts),'pureThrows':sum('threw'in x.get('outcome',{})for x in current),'stepThrows':sum('threw'in t.get('outcome',{})for t in ts),'rejections':sum('rejected'in s for t in ts for s in t['settled']),'reports':sum(len(t['reports'])for t in ts),'callbacks':sum(len(t['callbacks'])for t in ts),'console':sum(c['fn'].startswith('console.')for t in ts for c in t['callbacks']),'unmatched':sum(len(x.get('result',{}).get('unmatched',[]))for x in current),'namedDiagnostics':len(negative)})
write('integrity.json',{p.name:digest(p.read_bytes())for p in here.iterdir()if p.suffix in ['.json','.txt']and p.name!='integrity.json'})
print(json.dumps(read('accounting.json')['summary']));print(json.dumps(read('runtime-summary.json')));print('preserved',len(preserved),'source owners',len(paths))
