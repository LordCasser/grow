"""Alternating release CLI/PTY runs; rejects missing and empty test execution."""
import argparse,hashlib,json,os,re,subprocess
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument("test",type=Path);p.add_argument("before",type=Path);p.add_argument("after",type=Path);p.add_argument("--output",type=Path,required=True);p.add_argument("--runs",type=int,default=3);p.add_argument("--turns",default="128,512");p.add_argument("--slow-turns",default="512,2048");p.add_argument("--before-proxy",action="store_true");p.add_argument("--instrumentation-off",action="store_true");p.add_argument("--cases",nargs="+",default=["modern","slow","quit"]);a=p.parse_args()
a.output.mkdir(parents=True,exist_ok=True)
bins={s:getattr(a,s).resolve() for s in ("before","after")}
(a.output/"identities.json").write_text(json.dumps({s:{"path":str(b),"sha256":hashlib.sha256(b.read_bytes()).hexdigest()} for s,b in bins.items()},indent=2)+"\n")
rows=[]
for case in a.cases:
 name={"modern":"resume_modern_admission_projection_128_512_turns","slow":"resume_modern_history_progress_with_slow_writer","quit":"ctrl_d_exits_during_modern_session_loading"}[case]
 for repeat in range(1,(1 if case=="quit" else a.runs)+1):
  for stage in (["before","after"] if repeat%2 else ["after","before"]):
   stem=f"{stage}-{case}-{repeat}";log=a.output/f"{stem}.txt"
   env={k:v for k,v in os.environ.items() if not k.startswith("GROW_")}
   env.update(PAGER_BINARY=str(bins[stage]),RUST_MIN_STACK="16777216",GROW_PTY_RESUME_TRACE_DIR=str((a.output/stem).resolve()),GROW_PTY_RESUME_TURNS=a.turns if case=="modern" else (a.slow_turns if case=="slow" else "512"))
   if a.instrumentation_off:
    env["GROW_PTY_DISABLE_INSTRUMENTATION"]="1";env.pop("GROW_PTY_RESUME_TRACE_DIR",None)
   if stage=="before" and a.before_proxy:env["GROW_PTY_END_BATCH_PROXY"]="1"
   cmd=[str(a.test.resolve()),f"resume_input_latency::{name}","--exact","--ignored","--nocapture","--test-threads=1"]
   with log.open("w") as out:r=subprocess.run(cmd,env=env,stdout=out,stderr=subprocess.STDOUT,timeout=600)
   if r.returncode or not re.search(r"test result: ok\. 1 passed;",log.read_text()):raise SystemExit(f"{stem}: failed/empty; inspect {log}")
   for line in log.read_text().splitlines():
    if not any(tag in line for tag in ("[resume-pty]","[resume-progress]","[resume-phase]")):continue
    rows.append({"stage":stage,"case":case,"repeat":repeat,"source":log.name,"line":line})
   (a.output/"results.json").write_text(json.dumps(rows,indent=2)+"\n")
   print(stem,"passed",flush=True)
