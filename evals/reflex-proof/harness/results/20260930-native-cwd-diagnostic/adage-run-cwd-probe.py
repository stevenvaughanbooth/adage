import sys,json,time
from pathlib import Path
sys.path.insert(0,'evals/reflex-proof/harness')
import real_repo_pairs as p

g=p.substrate
root=Path('/private/tmp/adage-real-repo-gate-v1/cohorts/curve-v2/cwd-diagnostic-v1')
manifest=root/'manifest.json';m=p.read(manifest);authority=Path(m['experiment_root'])
for source,digest in m['source_sha256'].items():
 if p.sha(Path(source))!=digest: raise ValueError('diagnostic source drift')
g.validate_authority(m);g.require_native_review(m)
with p.execution_lock(authority):
 state=p.read(authority/'paid-state.json');key=str(root.relative_to(authority))
 if key in state['runs']:raise ValueError('no diagnostic retries')
 b=p.Budget(state,g.budget_limit(m),.15,persist=lambda:p.write(authority/'paid-state.json',state))
 attempt,reason=b.reserve(key)
 if attempt is None:raise ValueError(reason)
 state['runs'][key]=dict(status='reserved',kind=m['kind'],not_learning_evidence=True)
 p.write(authority/'paid-state.json',state)
 result=None
 try:
  if not b.dispatch(attempt):raise ValueError('accounting blocked dispatch')
  result=p.probe.execute(manifest,.15,180)
  b.settle(attempt,result)
  provenance=g.retain_transcript(root,Path(m['execution_root']))
  state['runs'][key].update(status='completed',summary=result,transcript=provenance)
 except BaseException as error:
  state['runs'][key].update(status='failed',error=str(error));raise
 finally:
  b.settle(attempt,result);p.write(authority/'paid-state.json',state)
  p.append(authority/'attempts.jsonl',dict(status='cwd-diagnostic-finished',run=key,accounting=attempt,evidence=state['runs'][key],timestamp_ns=time.time_ns()))
calls=[];results={}
for line in (root/'native-transcript.jsonl').read_text().splitlines():
 r=json.loads(line);content=r.get('message',{}).get('content',[])
 for block in content if isinstance(content,list) else []:
  if block.get('type')=='tool_use':calls.append(dict(id=block['id'],name=block['name'],command=block.get('input',{}).get('command'),recorded_cwd=r.get('cwd')))
  elif block.get('type')=='tool_result':results[block['tool_use_id']]=block
for call in calls:
 call['result']=results.get(call['id'])
report=dict(kind='native-cwd-metadata-diagnostic',expected_commands=m['commands'],calls=calls,exact_sequence=[c['command'] for c in calls]==m['commands'],reported_cost_usd=result.get('total_cost_usd'),native_sha256=provenance['sha256'],not_learning_evidence=True,not_efficacy_evidence=True)
p.write(root/'report.json',report)
print(json.dumps(report,indent=2))
