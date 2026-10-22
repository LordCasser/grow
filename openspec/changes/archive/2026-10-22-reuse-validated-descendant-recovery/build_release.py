import json,os,subprocess,sys,hashlib,shutil
from pathlib import Path
p=Path(__file__).resolve().parent;stage=sys.argv[1];groups=sys.argv[2:] or ["cli"]
env=os.environ.copy();env.update(CARGO_TARGET_DIR="target/descendant-recovery-release",CARGO_INCREMENTAL="0",CARGO_BUILD_JOBS="4")
identity=json.loads((p/f"{stage}-binaries.json").read_text()) if (p/f"{stage}-binaries.json").exists() else {}
for label,args in [("cli",["build","-p","cli","--bin","grow"]),("pager",["test","-p","pager","--lib","--test","pty_e2e_persistence","--no-run"]),("shell",["test","-p","shell","--features","test-support","--lib","--test","session_load_perf","--no-run"])]:
 if label not in groups:continue
 cmd=["cargo",*args,"--release","--locked","--offline","--message-format=json"]
 with (p/f"build-{stage}-{label}.jsonl").open("w") as out,(p/f"build-{stage}-{label}.log").open("w") as err:r=subprocess.run(cmd,env=env,stdout=out,stderr=err)
 if r.returncode:raise SystemExit(f"{stage} {label}: failed, inspect log")
 identity=json.loads((p/f"{stage}-binaries.json").read_text()) if (p/f"{stage}-binaries.json").exists() else {}
 for line in (p/f"build-{stage}-{label}.jsonl").read_text().splitlines():
  try:row=json.loads(line)
  except ValueError:continue
  if row.get("reason")=="compiler-artifact" and row.get("executable"):
   name=row["target"]["name"]
   if name not in ["grow","pager","pty_e2e_persistence","shell","session_load_perf"]:continue
   src=Path(row["executable"]);dest=Path("target/descendant-recovery-release/binaries")/f"{stage}-{name}";dest.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(src,dest)
   identity[name]={"path":str(dest.resolve()),"sha256":hashlib.sha256(dest.read_bytes()).hexdigest()}
 (p/f"{stage}-binaries.json").write_text(json.dumps(identity,indent=2)+"\n");print(stage,label,"built",flush=True)
