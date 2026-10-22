import json,os,re,subprocess,hashlib,statistics
from pathlib import Path
p=Path(__file__).resolve().parent;repo=next(root for root in p.parents if (root/"Cargo.toml").is_file());outdir=p/"semantic";outdir.mkdir(exist_ok=True)
bins={"before":(repo/"target/tui-scan-release/binaries/before-pager").resolve(),"after":(repo/"target/tui-scan-release/binaries/after-pager").resolve()}
(outdir/"identities.json").write_text(json.dumps({s:{"path":str(b),"sha256":hashlib.sha256(b.read_bytes()).hexdigest()} for s,b in bins.items()},indent=2)+"\n")
rows=[]
for repeat in range(1,4):
 for turns in [128,512,2048]:
  for stage in (["before","after"] if repeat%2 else ["after","before"]):
   log=outdir/f"{stage}-{turns}-{repeat}.log";env={k:v for k,v in os.environ.items() if not k.startswith("GROW_")};env.update(SEMANTIC_REPLAY_TURNS=str(turns),RUST_MIN_STACK="16777216")
   with log.open("w") as out:r=subprocess.run([str(bins[stage]),"app::acp_handler::tests::semantic_replay_bench::semantic_replay_without_drawing_scales_at_128_512_2048_turns","--exact","--ignored","--nocapture","--test-threads=1"],cwd=repo/"crates/codegen/pager",env=env,stdout=out,stderr=subprocess.STDOUT,timeout=300)
   text=log.read_text()
   if r.returncode or not re.search(r"test result: ok\. 1 passed;",text):raise SystemExit(f"failed or empty: {log}")
   line=next(line for line in text.splitlines() if "[semantic-replay-no-draw]" in line)
   values={k:float(v) for k,v in re.findall(r"(\w+)=([0-9]+(?:\.[0-9]+)?)",line)}
   rows.append(dict(stage=stage,repeat=repeat,source=log.name,**values));(outdir/"results.json").write_text(json.dumps(rows,indent=2)+"\n");print(stage,turns,repeat,values,flush=True)
summary=[]
for stage in bins:
 for turns in [128,512,2048]:
  selected=[r for r in rows if r["stage"]==stage and r["turns"]==turns];summary.append(dict(stage=stage,turns=turns,runs=len(selected),metrics={k:{"median":statistics.median(r[k] for r in selected),"range":[min(r[k] for r in selected),max(r[k] for r in selected)]} for k in selected[0] if isinstance(selected[0][k],float)}))
(outdir/"summary.json").write_text(json.dumps(summary,indent=2)+"\n")
