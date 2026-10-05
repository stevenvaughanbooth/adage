from pathlib import Path
import json, shutil, hashlib, sys, copy
repo=Path('/Users/stevenbooth/.codex/worktrees/adage-reflex-proof/skill-compile')
sys.path.insert(0,str(repo/'evals/reflex-proof/harness'))
import curve_transfer as transfer
root=Path('/private/tmp/adage-real-repo-gate-v1/cohorts/curve-guided-v2/transfer-v3')
report=transfer.report(root)
dest=repo/'evals/reflex-proof/harness/results/20260930-live-cpu-transfer'
dest.mkdir(exist_ok=False)
write=lambda p,d:p.write_text(json.dumps(d,indent=2)+'\n')
published=copy.deepcopy(report)
for row in published['rows']:
 for hook in row.get('hooks',[]):
  for key in ['before','after']:
   state=hook.get(key,{})
   if 'files' in state: state['archived_file_count']=len(state.pop('files'))
raw_report=max((root/'reports').glob('transfer-*.json'),key=lambda p:p.stat().st_mtime_ns)
published['serialization']={'omitted':'Repeated per-hook file maps only; source digests and all outcomes retained. Full original is in the hash-addressed local archive.','original_path':str(raw_report),'original_sha256':hashlib.sha256(raw_report.read_bytes()).hexdigest()}
write(dest/'report.json',published)
for n in ['manifest.json','manifest.sha256','checkpoint10-descriptor.json','approval-proposal.json']:
 shutil.copy2(root/n,dest/n)
rows=report['rows']; totals={}
for arm in transfer.ARMS:
 group=[r for r in rows if r['arm']==arm]
 totals[arm]={'assignments':len(group),'quality_passes':sum(r.get('quality',{}).get('passed') is True for r in group),'cost_usd':sum(r.get('cost_usd') or 0 for r in group),'known_costs':sum(r.get('cost_usd') is not None for r in group),'model_requests':sum(len(r.get('observed_dispatches',[])) for r in group),'agent_actions':sum(r.get('agent_action_count',0) for r in group),'execution_attempts':sum(len(r.get('execution_attempts',[])) for r in group),'journal_receipts':sum(len(r.get('all_journal_receipts',[])) for r in group)}
index=[]
for row in rows:
 session=root/'heldout'/row['task']/row['arm'];out=dest/'assignments'/row['task']/row['arm'];out.mkdir(parents=True)
 for n in ['manifest.json','run.json','transcript-provenance.json','final-source.json','final-tracked.patch']:
  p=session/n
  if p.is_file():shutil.copy2(p,out/n)
 retained={}
 for n in ['native-transcript.jsonl','stdout.jsonl','production-hooks.jsonl','production-session.spool','collection.json']:
  p=session/n
  if p.is_file():retained[str(p)]=hashlib.sha256(p.read_bytes()).hexdigest()
 index.append(dict(task=row['task'],arm=row['arm'],local_archive=retained))
write(dest/'archive-index.json',index)
# Post-hoc diagnostic only. The prospectively frozen score above remains unchanged.
diagnostics=[]
for row in rows:
 if row['arm']!='automatic': continue
 for hook in row.get('hooks',[]):
  for receipt in hook.get('journal_new',[]):
   changed=receipt.get('outcome',{}).get('changed',[])
   # Inspect the specific observed pytest cache files; do not generalize arbitrary hidden writes.
   names=[c.get('path','') for c in changed]
   cache_only=bool(names) and all(n in {'.pytest_cache/.gitignore','.pytest_cache/CACHEDIR.TAG','.pytest_cache/README.md','.pytest_cache/v/cache/nodeids','.pytest_cache/v/cache/lastfailed','.pytest_cache/v/cache/stepwise'} for n in names)
   same_source=hook.get('before',{}).get('digest')==row.get('final_generation')==hook.get('after',{}).get('digest')
   success=any(e.get('operation_id')==receipt.get('id') and e.get('exit')==0 and e.get('outcome')=='applied' for e in row.get('execution_attempts',[]))
   diagnostics.append(dict(task=row['task'],receipt_id=receipt.get('id'),approved_on=receipt.get('approved_on'),route_matches=receipt.get('route')==row.get('route'),session_matches=receipt.get('session')==row.get('provenance',{}).get('session_id'),source_matches_final=same_source,successful_runtime_attempt=success,changed_paths=names,only_enumerated_pytest_cache_files=cache_only,primary_score_unchanged=True))
write(dest/'runtime-diagnostic.json',dict(kind='post-hoc receipt audit; not a replacement gate',receipts=diagnostics,limitation='The frozen scorer rejects all changed paths, including approved caches. Runtime success does not establish agent action elimination.'))
write(dest/'summary.json',dict(passed=report['passed'],confirmed_eliminations=report['confirmed_eliminations'],totals=totals,limitations=report['limitations']))
delivery=[]
for row in rows:
 if row['arm']!='automatic': continue
 session=root/'heldout'/row['task']/row['arm']
 native=session/'native-transcript.jsonl'
 attachments=[]
 if native.exists():
  for line in native.read_text().splitlines():
   item=json.loads(line)
   if 'Adage ran' in line: attachments.append(item.get('attachment',{}).get('type'))
 receipts=[h for h in row.get('hooks',[]) if h.get('journal_new')]
 repeats=[]
 if receipts:
  end=min(h['ended_ns'] for h in receipts)
  for h in row.get('hooks',[]):
   p=h.get('payload',{}); command=p.get('tool_input',{}).get('command','')
   if h.get('event')=='PreToolUse' and p.get('tool_name')=='Bash' and h['started_ns']>end and transfer.pairs.command_matches(command,row['template']):
    repeats.append(dict(command=command,seconds_after_receipt=(h['started_ns']-end)/1e9,source_matches_final=h.get('before',{}).get('digest')==row.get('final_generation'),tool_use_id=p.get('tool_use_id')))
 delivery.append(dict(task=row['task'],native_receipt_attachment_types=attachments,receipt_hook_outputs=[h.get('output') for h in receipts],subsequent_matching_agent_requests=repeats))
write(dest/'delivery-audit.json',dict(kind='post-hoc native delivery and duplicate-request audit',rows=delivery))
shutil.copy2(root.parents[2]/'paid-state.json',dest/'budget-snapshot.json')
shutil.copy2(__file__,dest/'export.py')
print(json.dumps(dict(destination=str(dest),passed=report['passed'],confirmed=report['confirmed_eliminations'],totals=totals),indent=2))
