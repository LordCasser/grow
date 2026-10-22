import hashlib,json,os,re,subprocess
from pathlib import Path
p=Path(__file__).resolve().parent;b={k:v['path'] for k,v in json.loads((p/'after-binaries.json').read_text()).items()};env={k:v for k,v in os.environ.items() if not k.startswith('GROW_')};env.update(PAGER_BINARY=b['grow'],GROW_BINARY=b['grow'],RUST_MIN_STACK='16777216');rows=[]
tests=['suspend_nested_task_tree::ctrl_d_suspends_and_cold_resumes_root_child_grandchild_tree','suspend_nested_task_tree::cold_resume_after_partial_leaf_release_does_not_replay_checkpoint','suspend_nested_task_tree::concurrent_cold_resume_claims_one_root_without_replaying_tree_work','continue_resumes_session_with_history::continue_resumes_session_with_history']
for index,test in enumerate(tests):
 log=p/f'manual-{index}.log'
 with log.open('w') as out:r=subprocess.run([b['pty_e2e_persistence'],test,'--exact','--ignored','--nocapture','--test-threads=1'],env=env,stdout=out,stderr=subprocess.STDOUT,timeout=240)
 txt=log.read_text();ok=r.returncode==0 and bool(re.search(r'test result: ok\. 1 passed;',txt));row=dict(test=test,exit=r.returncode,ok=ok,log=log.name,cli_sha256=hashlib.sha256(Path(b['grow']).read_bytes()).hexdigest());rows.append(row);(p/'manual-checks.json').write_text(json.dumps(rows,indent=2)+'\n');print(row,flush=True)
 if not ok:raise SystemExit(1)
