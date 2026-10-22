import json,os,re,subprocess,sys
from pathlib import Path
p=Path(__file__).resolve().parent;b={name: row["path"] for name,row in json.loads((p/"binary-identity.json").read_text()).items()};b["pager_render"]=str(Path("target/resume-attribution-release/binaries/p0-pager_render").resolve());env=os.environ.copy();env["RUST_MIN_STACK"]="16777216"
groups={"pager":["app::acp_handler::tests::","acp::tracker::tests::","app::root::dispatch::tests::session::load::","app::root::dispatch::tests::session::fork::","app::root::event_loop::tests::","scrollback::","views::block_viewer::"],"pager_render":["render::draw::tests::","prompt_images::tests::"],"markdown":[""]}
results=json.loads((p/"checks.json").read_text()) if (p/"checks.json").exists() else []
for binary,filters in groups.items():
 if len(sys.argv)>1 and binary not in sys.argv[1:]:continue
 for index,group in enumerate(filters):
  if any(row.get("ok") and row["binary"]==binary and row["filter"]==group for row in results):continue
  log=p/f"checks-{binary}-{index}.log"
  excluded=["scrollback::state::tests::insert_block_before_rejects_an_already_committed_anchor"] if binary=="pager" and group=="scrollback::" else []
  skip_args=[arg for name in excluded for arg in ["--skip",name]]
  with log.open("w") as out:r=subprocess.run([b[binary],group,"--test-threads=1",*skip_args],env=env,stdout=out,stderr=subprocess.STDOUT,timeout=900)
  matches=list(re.finditer(r"test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored",log.read_text()));m=matches[-1] if matches else None
  row={"binary":binary,"filter":group,"log":log.name,"exit":r.returncode,"passed":int(m[2]) if m else 0,"failed":int(m[3]) if m else None,"ignored":int(m[4]) if m else None};row["release_debug_assert_only_exclusions"]=excluded;row["ok"]=r.returncode==0 and row["passed"]>0;results.append(row)
  (p/"checks.json").write_text(json.dumps(results,indent=2)+"\n");print(row,flush=True)
  if not row["ok"]:raise SystemExit(1)
