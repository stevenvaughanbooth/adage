from pathlib import Path
import sys,json,os,subprocess,shutil,time
repo=Path('/Users/stevenbooth/.codex/worktrees/adage-reflex-proof/skill-compile')
sys.path.insert(0,str(repo/'evals/reflex-proof/harness'))
import curve_sessions as sessions
import curve_replay
from real_repo_pairs import read,write,sha
r=Path('/private/tmp/adage-real-repo-gate-v1/cohorts/curve-guided-v2')
p=read(r/'cumulative-protocol.json'); m=read(r/'manifest.json'); binary=r/'bin/adage'
assert sha(r/'manifest.json')==p['new_cohort_manifest_sha256']
assert sha(binary)==p['source_binary_sha256']
assert sha(Path(p['old_checkpoint']))==p['old_attempt_sha256']
shutil.copy2(__file__,r/'run-driver.py')
sessions.gate.run_session(r/'sessions/native-confinement/manifest.json',wall_seconds=600)
review=sessions.review_native(r); print(json.dumps(dict(native_review=review['passed'])),flush=True)
sessions.checkpoint(r,0,binary)
for task in p['new_tasks']:
 path=sessions.prepare_session(r,task)
 result=sessions.gate.run_session(path,wall_seconds=600)
 print(json.dumps(dict(task=task,status='terminal-recorded')),flush=True)
new=sessions.checkpoint(r,5,binary)
old=read(Path(p['old_checkpoint']))
rows=curve_replay.validated_inputs(dict(status='completed',count=10,inputs=old['inputs']+new['inputs']))
out=r/'cumulative-10'; out.mkdir(exist_ok=False); home=out/'adage-home'; history=out/'history'; history.mkdir()
inputs=[]
for row in rows:
 target=history/Path(row['copy']).name; shutil.copy2(row['copy'],target)
 assert sha(target)==row['sha256']; inputs.append(dict(row,copy=str(target)))
command=[str(binary),'condition','--plain','--json','--transcripts',*[row['copy'] for row in inputs]]
env={k:v for k,v in os.environ.items() if not k.startswith('ADAGE_')}; env.update(ADAGE_HOME=str(home),ADAGE_AUTO_LEARN='0')
a=dict(kind='cumulative-guided-development',status='started',protocol_sha256=sha(r/'cumulative-protocol.json'),count=10,distinct_tasks=5,inputs=inputs,command=command,binary_sha256=sha(binary),prior_attempt_sha256=sha(Path(p['old_checkpoint'])),new_attempt_sha256=sha(r/'checkpoints/5/attempt.json'),paid_conditioning_calls=0,grants_written=False,efficacy_result=False,production_source_sha256={str(x.relative_to(repo)):sha(x) for x in [*sorted((repo/'crates').rglob('*')),repo/'Cargo.toml',repo/'Cargo.lock'] if x.is_file()})
write(out/'attempt.json',a)
start=time.monotonic_ns(); result=subprocess.run(command,cwd=out,env=env,capture_output=True,text=True,timeout=600)
(out/'condition.stdout.json').write_text(result.stdout); (out/'condition.stderr.txt').write_text(result.stderr)
a.update(status='completed' if result.returncode==0 else 'failed',exit_code=result.returncode,elapsed_ns=time.monotonic_ns()-start)
if result.returncode==0:
 index=read(home/'index.json'); a.update(route_count=len(index['routes']),execution_candidates=[x for x in index['routes'] if x['route'].get('evidence',{}).get('execution')])
 a['artifact_sha256']={str(x.relative_to(home)):sha(x) for x in [home/'index.json',*sorted((home/'tracks').rglob('*'))] if x.is_file()}
assert not (home/'approvals.json').exists(); assert sha(binary)==p['source_binary_sha256']; write(out/'attempt.json',a)
print(json.dumps(dict(count=10,status=a['status'],execution_candidates=len(a.get('execution_candidates',[])))),flush=True)
