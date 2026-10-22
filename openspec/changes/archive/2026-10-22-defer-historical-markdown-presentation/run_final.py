import json,os,subprocess,re,hashlib,shutil
from pathlib import Path
p=Path(__file__).resolve().parent;root=Path.cwd();base=Path("openspec/changes/archive/2026-10-22-profile-resume-presentation-attribution")
identities=json.loads((p/"binary-identity.json").read_text());test=identities["pty_e2e_persistence"]["path"];before=str((root/"target/resume-attribution-release/binaries/p0-grow"));after=identities["grow"]["path"]
old=json.loads((p/"checks.json").read_text());(p/"pre-nav-checks.json").write_text(json.dumps(old,indent=2)+"\n") if not (p/"pre-nav-checks.json").exists() else None;(p/"checks.json").write_text(json.dumps([r for r in old if r["binary"]!="pager"],indent=2)+"\n")
def run(args):
 print("RUN",args,flush=True);subprocess.run(args,check=True)
run(["python3",str(p/"run_checks.py"),"pager"])
run(["python3",str(p/"run_pty_pairs.py"),test,before,after,"--output",str(p/"warmup"),"--runs","1","--turns","128","--slow-turns","512,2048","--cases","modern","slow"])
run(["python3",str(p/"run_pty_pairs.py"),test,before,after,"--output",str(p/"default-paired"),"--turns","128,512,2048","--instrumentation-off"])
run(["python3",str(p/"summarize_metrics.py"),str(p/"default-paired")])
run(["python3",str(p/"run_pty_pairs.py"),test,before,after,"--output",str(p/"paired"),"--turns","128,512,2048"])
run(["python3",str(p/"summarize_metrics.py"),str(p/"paired")])
run(["python3",str(base/"analyze_pty.py"),str(p/"paired")])
run(["python3",str(p/"run_semantic_pairs.py")])
for stage,binary in [("before",before),("after",after)]:
 env={k:v for k,v in os.environ.items() if not k.startswith("GROW_")};env.update(PAGER_BINARY=binary,RUST_MIN_STACK="16777216")
 log=p/f"followup-{stage}.log"
 with log.open("w") as out:r=subprocess.run([test,"continue_resumes_session_with_history::continue_resumes_session_with_history","--exact","--ignored","--nocapture","--test-threads=1"],env=env,stdout=out,stderr=subprocess.STDOUT,timeout=300)
 if r.returncode or not re.search(r"test result: ok\. 1 passed;",log.read_text()):raise SystemExit(f"{stage}: followup failed/empty")
 print("followup",stage,"passed",flush=True)
print("FINAL RUNS COMPLETE",flush=True)
