"""Sequential real CLI/PTY checks; compare paired processes without builds."""
import argparse,hashlib,json,os,re,subprocess
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('test',type=Path);p.add_argument('before',type=Path);p.add_argument('after',type=Path);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
a.output.mkdir(parents=True,exist_ok=True)
bins={'before':a.before.resolve(),'after':a.after.resolve()}
(a.output/'pty-binaries.json').write_text(json.dumps({k:{'path':str(v),'sha256':hashlib.sha256(v.read_bytes()).hexdigest()} for k,v in bins.items()},indent=2)+'\n')
rows=[]
for case in ['modern','slow','quit']:
 name={'modern':'resume_modern_admission_projection_128_512_turns','slow':'resume_modern_history_progress_with_slow_writer','quit':'ctrl_d_exits_during_modern_session_loading'}[case]
 for repeat in range(1,4 if case=='modern' else 2):
  for stage in (['before','after'] if repeat%2 else ['after','before']):
   log=a.output/f'{stage}-pty-{case}-{repeat}.txt'
   env={k:v for k,v in os.environ.items() if not k.startswith('GROW_')};env.update(PAGER_BINARY=str(bins[stage]),RUST_MIN_STACK='16777216')
   if stage=='before':env['GROW_PTY_LEGACY_COMPARISON']='1'
   cmd=[str(a.test.resolve()),f'resume_input_latency::{name}','--exact','--ignored','--nocapture','--test-threads=1']
   with log.open('w') as f:r=subprocess.run(cmd,env=env,stdout=f,stderr=subprocess.STDOUT,timeout=300)
   content=log.read_text()
   if r.returncode or not re.search(r'test result: ok\. 1 passed;',content):raise SystemExit(f'{log}: failed or empty')
   for line in content.splitlines():
    if '[resume-pty]' not in line:continue
    row={'stage':stage,'case':case,'repeat':repeat,'source':log.name}
    for key,val in re.findall(r'([a-z0-9_]+)=([^ ]+)',line):
     try:row[key]=float(val)
     except ValueError:row[key]=val
    rows.append(row)
   (a.output/'pty-measurements.json').write_text(json.dumps(rows,indent=2)+'\n')
   print(f'{stage} {case} {repeat}: passed',flush=True)
