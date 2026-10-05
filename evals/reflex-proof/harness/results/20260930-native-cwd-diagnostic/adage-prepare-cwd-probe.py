import sys,json,shutil
from pathlib import Path
sys.path.insert(0,'evals/reflex-proof/harness')
import curve_sessions as s
import cohort_v5 as native
import real_repo_pairs as p

g=p.substrate
cohort=Path('/private/tmp/adage-real-repo-gate-v1/cohorts/curve-v2')
authority=cohort.parent.parent
root=cohort/'cwd-diagnostic-v1'
root.mkdir(exist_ok=False)
base=p.read(cohort/'manifest.json');plan=base['families'][0]
checkout=root/'repo'
g.create_checkout(cohort,plan['repo'],plan['base_full'],checkout)
p.frozen_setup(cohort,checkout,plan,base['python_requested'])
settings=native.policy(authority,checkout,cohort/'runtime/python')
p.write(root/'settings.json',settings)
(root/'.reflex-fixture').write_text('native cwd metadata diagnostic; never a learning trace\n')
commands=['pwd','cd tests','pwd','cd ..','pwd','.venv/bin/python -c \'import os; print(os.getcwd())\'']
prompt='This is a shell working-directory diagnostic, not a coding task. Run exactly these six Bash commands in order, each in a separate tool call and assistant turn. Do not combine them. Do not inspect or change other files. After all six, finish with a short confirmation. Commands: '+json.dumps(commands)
argv=['claude','--print','--restricted','--model',base['model'],'--max-budget-usd','0.15','--max-turns','8','--output-format','stream-json','--verbose','--debug-file',str(root/'claude-debug.log'),'--no-chrome','--disable-slash-commands','--setting-sources','','--settings',str(root/'settings.json'),'--strict-mcp-config','--mcp-config','{"mcpServers":{}}','--tools','Bash','--permission-mode','dontAsk','--permission-prompts','none',prompt]
m=dict(root=str(root),experiment_root=str(authority),cohort_root=str(cohort),execution_root=str(checkout),kind='native-cwd-metadata-diagnostic',model=base['model'],commands=commands,argv=native.startup_argv(argv),budget_usd=.15,expected_claude_version=p.probe.EXPECTED_VERSION,budget_allocation_sha256=base['budget_allocation_sha256'],native_review_path=str(cohort/'native-confinement-reviewed.json'),native_review_sha256=p.sha(cohort/'native-confinement-reviewed.json'),source_sha256={str(x):p.sha(x) for x in [Path(__file__).resolve(),Path(g.__file__),Path(p.__file__),Path(native.__file__),root/'settings.json']},not_learning_evidence=True,not_efficacy_evidence=True)
g.validate_authority(m);g.require_native_review(m);g.budget_limit(m)
p.write(root/'manifest.json',m)
print(root)
