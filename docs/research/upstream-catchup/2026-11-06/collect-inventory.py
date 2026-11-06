import subprocess, json, pathlib, datetime

root = pathlib.Path(subprocess.check_output(['git', '-C', str(pathlib.Path(__file__).resolve().parent), 'rev-parse', '--show-toplevel'], text=True).strip())
out = root / 'docs/research/upstream-catchup/2026-11-06'
out.mkdir(parents=True, exist_ok=True)
target = '8297bad8d388eb9c70324c22cb001c59248f95f4'
old = '4ad8986099314b9587df423c802db103a21d665d'
runtime = pathlib.Path(subprocess.check_output(['git', '-C', str(root), 'rev-parse', '--path-format=absolute', '--git-common-dir'], text=True).strip()) / 'catchup-runtime.git'
def git(*args, repo=root):
    return subprocess.check_output(['git','-C',str(repo),*args],text=True).strip()
def record(name, *args, repo=root):
    result = git(*args,repo=repo)
    (out/name).write_text(result+'\n')
    return result
branches={}
for branch in ['research/multi-platform-rust-core-impl','research/multi-platform-rust-core-impl-plugins']:
    sha=git('rev-parse','origin/'+branch)
    base=git('merge-base',sha,target)
    branches[branch]={'sha':sha,'commit':git('show','-s','--format=%cI %s',sha),'mergeBase':base,'leftRightCounts':git('rev-list','--left-right','--count',sha+'...'+target),'runtimeGitlink':git('ls-tree',sha,'packages/engine/runtime/runtime-src').split()[2]}
    short='impl-plugins' if branch.endswith('plugins') else 'impl'
    record(short+'-upstream-paths.tsv','diff','--name-status',base,target)
    record(short+'-private-paths.tsv','diff','--name-status',base,sha)
    record(short+'-endpoint-stat.txt','diff','--stat',sha,target)
    a=set(git('diff','--name-only',base,sha).splitlines()); b=set(git('diff','--name-only',base,target).splitlines())
    branches[branch]['overlappingPaths']=sorted(a&b)
    branches[branch]['upstreamChangedPaths']=len(b)
    record(short+'-overlap.txt','diff','--stat',base,target,'--',*sorted(a&b))
    record(short+'-rust-owners.txt','ls-tree','-r','--name-only',sha,'crates','platforms','tooling')
record('viewer-since-c7-commits.tsv','log','--reverse','--format=%H\t%cI\t%s',old+'..'+target)
record('viewer-since-c7-paths.tsv','diff','--name-status',old,target)
record('viewer-since-c7-stat.txt','diff','--stat',old,target)
record('viewer-functional.diff','diff',old,target,'--','packages/engine','packages/core','packages/plugin','packages/web','packages/viewer','packages/react','packages/angular')
record('viewer-since-baseline-commits.tsv','log','--reverse','--format=%H\t%cI\t%s',next(iter(branches.values()))['mergeBase']+'..'+target)
record('branch-divergence.txt','log','--left-right','--oneline','origin/research/multi-platform-rust-core-impl...origin/research/multi-platform-rust-core-impl-plugins')
record('runtime-since-c7.diff','diff','eeac787c02fac7b40c74c91bbff0ffb5f59ff97e','4346c5131cd009c4086fdce13bde7fd43fbd4ce0',repo=runtime)
record('runtime-since-c7-commits.tsv','log','--reverse','--format=%H\t%cI\t%s','eeac787c..4346c513',repo=runtime)
for short,sha in [('impl',branches['research/multi-platform-rust-core-impl']['runtimeGitlink']),('impl-plugins',branches['research/multi-platform-rust-core-impl-plugins']['runtimeGitlink'])]:
    record(short+'-runtime-fork-commits.tsv','log','--format=%H\t%s','4346c5131cd009c4086fdce13bde7fd43fbd4ce0..'+sha,repo=runtime)
    record(short+'-runtime-upstream-commits.tsv','log','--format=%H\t%s',sha+'..4346c5131cd009c4086fdce13bde7fd43fbd4ce0',repo=runtime)
pins={'scope':'Planning only; no viewer/runtime merge or implementation authorized','observedUTC':datetime.datetime.now(datetime.timezone.utc).isoformat(),'workingRepository':'https://github.com/roanutil/embed-pdf-viewer.git','viewer':{'canonicalRepo':'https://github.com/embedpdf/embed-pdf-viewer.git','branch':'main','sha':target,'commit':git('show','-s','--format=%cI %s',target),'runtimeGitlink':git('ls-tree',target,'packages/engine/runtime/runtime-src').split()[2]},'runtime':{'canonicalRepo':'https://github.com/embedpdf/runtime.git','branch':'embedpdf/main','observedHead':'4346c5131cd009c4086fdce13bde7fd43fbd4ce0','decision':'Required fixed runtime target4346c5131cd009c4086fdce13bde7fd43fbd4ce0; C7 A/B superseded'},'branches':branches,'previousC7Target':old,'sinceC7':{'commits':git('rev-list','--count',old+'..'+target),'nonMergeCommits':git('rev-list','--no-merges','--count',old+'..'+target),'changedPaths':len(git('diff','--name-only',old,target).splitlines())},'container':{'name':'embed-pdf','checkout':str(root),'volume':'embedpdf-workspace','image':'sha256:30b97dec3bd8d007b9197ab8fc0027f3b62ea3bd25390a5f516b4cbab085df84'},'branchDivergence':git('rev-list','--left-right','--count','origin/research/multi-platform-rust-core-impl...origin/research/multi-platform-rust-core-impl-plugins')}
(out/'pinned-targets.json').write_text(json.dumps(pins,indent=2)+'\n')
print(json.dumps(pins,indent=2))
