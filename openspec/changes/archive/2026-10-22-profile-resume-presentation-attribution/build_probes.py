import json,os,subprocess,sys
from pathlib import Path
p=Path(__file__).resolve().parent
env=os.environ.copy();env.update(CARGO_TARGET_DIR="target/resume-attribution-release",CARGO_INCREMENTAL="0",CARGO_BUILD_JOBS="4")
artifacts=json.loads((p/"probe-binaries.json").read_text()) if (p/"probe-binaries.json").exists() else {}
for label,args in [("pager",["-p","pager","--lib","--test","pty_e2e_persistence"]),("rpc",["-p","shell","--features","test-support","--test","session_load_perf"]),("render",["-p","pager-render","--lib"])]:
 if len(sys.argv)>1 and label not in sys.argv[1:]:continue
 cmd=["cargo","test","--release","--locked","--offline",*args,"--no-run","--message-format=json"]
 with (p/f"build-{label}.jsonl").open("w") as out,(p/f"build-{label}.log").open("w") as err:
  r=subprocess.run(cmd,env=env,stdout=out,stderr=err)
 if r.returncode:raise SystemExit(f"{label} build failed; inspect build-{label}.log")
 for line in (p/f"build-{label}.jsonl").read_text().splitlines():
  try:row=json.loads(line)
  except ValueError:continue
  if row.get("reason")=="compiler-artifact" and row.get("executable") and row.get("profile",{}).get("test"):
   artifacts[row["target"]["name"]]=row["executable"]
 (p/"probe-binaries.json").write_text(json.dumps(artifacts,indent=2)+"\n")
 print(label,"built",flush=True)
