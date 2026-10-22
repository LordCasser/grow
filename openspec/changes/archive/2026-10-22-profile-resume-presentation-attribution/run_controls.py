import json,os,re,subprocess
from pathlib import Path
p=Path(__file__).resolve().parent
bin_dir=Path('target/resume-attribution-release/binaries').resolve()
for repeat in range(1,4):
 for turns in [128,512,2048]:
  env={k:v for k,v in os.environ.items() if not k.startswith('GROW_')};env.update(SEMANTIC_REPLAY_TURNS=str(turns),RUST_MIN_STACK='16777216')
  log=p/f'semantic-{turns}-{repeat}.log'
  with log.open('w') as out:r=subprocess.run([str(bin_dir/'p0-pager'),'app::acp_handler::tests::semantic_replay_bench::semantic_replay_without_drawing_scales_at_128_512_2048_turns','--exact','--ignored','--nocapture','--test-threads=1'],env=env,stdout=out,stderr=subprocess.STDOUT,timeout=300)
  if r.returncode or not re.search(r'test result: ok\. 1 passed;',log.read_text()):raise SystemExit(f'{log}: failed or empty')
  print('semantic',turns,repeat,'passed',flush=True)
script=Path('openspec/changes/archive/2026-10-22-profile-release-resume-first-history/run_matrix.py')
for stage,extra in [('control',[]),('oracle',['--correctness'])]:
 subprocess.run(['python3',str(script),str(bin_dir/'p0-session_load_perf'),'--output',str(p/'rpc'),'--stage',stage,'--runs','3','--profiles','modern-128','modern-512','modern-2048',*extra],check=True)
for stage in ['pre-p0','p0']:
 env=os.environ.copy();env.update(PAGER_BINARY=str(bin_dir/f'{stage}-grow'),RUST_MIN_STACK='16777216')
 log=p/f'followup-{stage}.log'
 with log.open('w') as out:r=subprocess.run([str(bin_dir/'p0-pty_e2e_persistence'),'continue_resumes_session_with_history::continue_resumes_session_with_history','--exact','--ignored','--nocapture','--test-threads=1'],env=env,stdout=out,stderr=subprocess.STDOUT,timeout=300)
 if r.returncode or not re.search(r'test result: ok\. 1 passed;',log.read_text()):raise SystemExit(f'{log}: failed or empty')
 print('followup',stage,'passed',flush=True)
