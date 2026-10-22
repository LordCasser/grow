import json,os,subprocess,hashlib,shutil
from pathlib import Path
p=Path(__file__).resolve().parent
env=os.environ.copy();env.update(CARGO_TARGET_DIR="target/resume-attribution-release",CARGO_INCREMENTAL="0",CARGO_BUILD_JOBS="4")
artifacts=json.loads((p/"binary-identity.json").read_text()) if (p/"binary-identity.json").exists() else {}
for label,args in [("cli",["build","-p","cli","--bin","grow"]),("probes",["test","-p","pager","--lib","--test","pty_e2e_persistence","--no-run"]),("unit",["test","-p","pager","--lib","--no-run"]),("pty",["test","-p","pager","--test","pty_e2e_persistence","--no-run"]),("markdown",["test","-p","markdown","--lib","--no-run"])]:
 if len(__import__("sys").argv)>1 and label not in __import__("sys").argv[1:]:continue
 cmd=["cargo",*args,"--release","--locked","--offline","--message-format=json"]
 with (p/f"build-{label}.jsonl").open("w") as out,(p/f"build-{label}.log").open("w") as err:r=subprocess.run(cmd,env=env,stdout=out,stderr=err)
 if r.returncode:raise SystemExit(f"{label} failed; inspect log")
 for line in (p/f"build-{label}.jsonl").read_text().splitlines():
  try:row=json.loads(line)
  except ValueError:continue
  if row.get("reason")=="compiler-artifact" and row.get("executable"):
   name=row["target"]["name"];src=Path(row["executable"]);dst=Path("target/resume-attribution-release/binaries")/f"p1-{name}"
   shutil.copy2(src,dst);artifacts[name]={"path":str(dst.resolve()),"sha256":hashlib.sha256(dst.read_bytes()).hexdigest()}
 (p/"binary-identity.json").write_text(json.dumps(artifacts,indent=2)+"\n")
 print(label,"built",flush=True)
