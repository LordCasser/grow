"""Fresh-process release AB/BA pairs, fixed validated star/chain fixtures."""
import argparse,hashlib,json,os,re,subprocess
from pathlib import Path
from statistics import median
p=argparse.ArgumentParser();p.add_argument('before',type=Path);p.add_argument('after',type=Path);p.add_argument('--output',type=Path,required=True);p.add_argument('--runs',type=int,default=3);a=p.parse_args();a.output.mkdir(parents=True,exist_ok=True)
bins={k:getattr(a,k).resolve() for k in ('before','after')};rows=[]
(a.output/'identities.json').write_text(json.dumps({k:{'path':str(v),'sha256':hashlib.sha256(v.read_bytes()).hexdigest()} for k,v in bins.items()},indent=2)+'\n')
for repeat in range(a.runs+1):
 for stage in (['before','after'] if repeat%2 else ['after','before']):
  label=f'{stage}-{repeat}';log=a.output/f'{label}.log';env={k:v for k,v in os.environ.items() if not k.startswith('GROW_')};env['RUST_MIN_STACK']='16777216'
  with log.open('w') as out:r=subprocess.run([str(bins[stage]),'app::acp_handler::tests::descendant_recovery_bench::descendant_recovery_star_and_chain_release_benchmark','--exact','--ignored','--nocapture','--test-threads=1'],env=env,stdout=out,stderr=subprocess.STDOUT,timeout=300)
  txt=log.read_text()
  if r.returncode or 'test result: ok. 1 passed;' not in txt:raise SystemExit(f'failed/empty: {log}')
  found=[json.loads(line) for line in txt.splitlines() if line.startswith('{"benchmark":"descendant_recovery"')]
  if len(found)!=2:raise SystemExit(f'missing shapes: {log}')
  for row in found:row.update(stage=stage,repeat=repeat,warmup=repeat==0,source=log.name);rows.append(row)
  (a.output/'results.json').write_text(json.dumps(rows,indent=2)+'\n');print(label,'passed',flush=True)
summary=[]
for shape in ('star','chain'):
 for stage in bins:
  values=[r['recovery_ms'] for r in rows if r['shape']==shape and r['stage']==stage and not r['warmup']]
  summary.append(dict(shape=shape,stage=stage,n=len(values),median_ms=median(values),min_ms=min(values),max_ms=max(values)))
(a.output/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
