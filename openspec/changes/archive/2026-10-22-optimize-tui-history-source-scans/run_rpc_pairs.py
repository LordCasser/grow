"""Alternate retained release backend controls, with complete-history oracle."""
import json,subprocess,sys
from pathlib import Path
from statistics import median
p=Path(__file__).resolve().parent;repo=next(root for root in p.parents if (root/'Cargo.toml').is_file() and (root/'openspec').is_dir());out=p/'rpc-pairs';out.mkdir(exist_ok=True);runner=repo/'openspec/changes/archive/2026-10-22-profile-release-resume-first-history/run_matrix.py'
for repeat in range(1,4):
 for stage in (['before','after'] if repeat%2 else ['after','before']):
  binary=Path(f'target/tui-scan-release/binaries/{stage}-session_load_perf').resolve()
  cmd=[sys.executable,str(runner),str(binary),'--output',str(out),'--stage',f'{stage}-{repeat}','--runs','1','--correctness','--profiles','modern-128','modern-2048']
  subprocess.run(cmd,check=True,cwd=repo/'crates/codegen/shell')
rows=[]
for f in sorted(out.glob('*-measurements.json')):
 for r in json.loads(f.read_text()):r['stage_group']=r['stage'].split('-')[0];rows.append(r)
(out/'results.json').write_text(json.dumps(rows,indent=2)+'\n');summary=[]
for profile in ['modern-128','modern-2048']:
 for stage in ['before','after']:
  rs=[r for r in rows if r['stage_group']==stage and r['profile']==profile];item=dict(profile=profile,stage=stage,n=len(rs))
  for key in ['load_ms','first_history_ms','last_history_ms']:
   vals=[r[key] for r in rs];item[key]=dict(median=median(vals),min=min(vals),max=max(vals))
  summary.append(item)
(out/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
