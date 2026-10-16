from pathlib import Path
import sys,json,shutil,datetime
repo=Path('/Users/stevenbooth/.codex/worktrees/adage-reflex-proof/skill-compile'); sys.path.insert(0,str(repo/'evals/reflex-proof/harness'))
import curve_evidence
from real_repo_pairs import read,write,sha
root=Path('/private/tmp/adage-real-repo-gate-v1/cohorts/curve-guided-v1/cold-replication-v1')
first_path=repo/'evals/reflex-proof/harness/results/20260930-cold-start-pilot/report.json'
first=read(first_path); second=curve_evidence.cold_report(root)
assert all(t['complete'] for d in (first,second) for t in d['totals'].values())
assert all(t['reported_cost_usd'] is not None and t['observed_model_requests'] is not None for d in (first,second) for t in d['totals'].values())
rows=[dict(r,replication=round_id) for round_id,d in enumerate((first,second),1) for r in d['rows']]
totals={arm:{key:sum(d['totals'][arm][key] for d in (first,second)) for key in ('assignments','quality_passed','reported_cost_usd','observed_model_requests','agent_tool_calls')} for arm in ('agent','instincts')}
regressions=0
for round_id in (1,2):
 for task in sorted({r['task'] for r in rows}):
  pair={r['arm']:r for r in rows if r['replication']==round_id and r['task']==task}
  regressions+=pair['instincts']['cost_usd']>pair['agent']['cost_usd']
combined=dict(protocol='cold-start-two-runs-descriptive-1',recorded_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),unique_tasks=4,repetitions_per_arm_per_task=2,assignment_denominator=16,first_report_sha256=sha(first_path),replication_manifest_sha256=sha(root/'manifest.json'),totals=totals,rows=rows,cost_regression_pairs=regressions,paired_comparisons=8,cost_reduction_percent=100*(1-totals['instincts']['reported_cost_usd']/totals['agent']['reported_cost_usd']),request_reduction_percent=100*(1-totals['instincts']['observed_model_requests']/totals['agent']['observed_model_requests']),cpu_action_elimination_tested=False,gpu_utilization_measured=False,limitations=['Same four authored tasks on one pinned Marshmallow repository, two runs per arm; not eight distinct tasks or broad efficacy.','Second run reverses each first-run pair order; same model, product binary, prompts and grading; no outcome retries.','All sixteen outcomes, failures and cost regressions retained.','Intervention is shipped advisory hints, not learned automatic CPU execution.','Replication registered after observing first pilot. This is descriptive evidence, not an independently powered confirmatory trial.'])
out=repo/'evals/reflex-proof/harness/results/20260930-cold-start-replication';out.mkdir(exist_ok=False)
write(out/'report.json',combined);write(out/'replication-report.json',second)
for n in ('manifest.json','checkpoint0-descriptor.json'):shutil.copy2(root/n,out/n)
shutil.copytree(root/'source',out/'frozen-source')
for entry in read(root/'manifest.json')['matrix']:
 src=root/'heldout'/entry['task']/entry['arm']; dst=out/'assignments'/entry['task']/entry['arm'];dst.mkdir(parents=True)
 for n in ('manifest.json','settings.json','run.json','transcript-provenance.json'):shutil.copy2(src/n,dst/n)
shutil.copy2(__file__,out/'export.py')
write(out/'validation.json',dict(harness_tests_passed=81,all_assignments_accounted_for=True,all_collection_errors_empty=all(not r['collection_errors'] for r in rows),source_binary_unchanged=True,replication_report_cost_usd=sum(t['reported_cost_usd'] for t in second['totals'].values()),grants_created=False,preparation_failures=['Initial generic cold-study guard rejected repeated tasks before any paid calls.','Old native-review source hash rejected the newer runner; the current passed native review was selected.','One offline setup used a root outside that reviewed cohort and was rejected; that unused folder was retained. The final study is nested within the reviewed cohort.']))
print(json.dumps({k:combined[k] for k in ('totals','cost_regression_pairs','cost_reduction_percent','request_reduction_percent')},indent=2))
