"""Scoped release regressions; reject empty selections and retain failures."""
import json,os,re,subprocess,sys,hashlib
from pathlib import Path
p=Path(__file__).resolve().parent;repo=next(root for root in p.parents if (root/'Cargo.toml').is_file());b={k:v['path'] for k,v in json.loads((p/'after-binaries.json').read_text()).items()};env=os.environ.copy();env['RUST_MIN_STACK']='16777216';env['GROW_BINARY']=b['grow'];env['PAGER_BINARY']=b['grow']
groups={'pager':['app::acp_handler::tests::','acp::meta::tests::','acp::tracker::tests::','app::root::dispatch::tests::session::load::','app::root::dispatch::tests::session::fork::','app::root::event_loop::tests::','app::subagent::tests::','scrollback::','views::block_viewer::'],'pager_render':['prompt_images::tests::','render::draw::tests::'],'shell':['session::storage::','session::history::','session::persistence::','agent::mvp_agent::runtime_handoff::tests::','agent::subagent::']}
results=json.loads((p/'checks.json').read_text()) if (p/'checks.json').exists() else []
for binary,filters in groups.items():
 if len(sys.argv)>1 and binary not in sys.argv[1:]:continue
 for index,group in enumerate(filters):
  binary_sha=hashlib.sha256(Path(b[binary]).read_bytes()).hexdigest()
  if any(row.get('ok') and row['binary']==binary and row['filter']==group and row.get('binary_sha256')==binary_sha for row in results):continue
  log=p/f'checks-{binary}-{index}.log';excluded=['scrollback::state::tests::insert_block_before_rejects_an_already_committed_anchor'] if group=='scrollback::' else []
  if log.exists():
   attempt=1
   while log.with_name(log.stem+f'-attempt-{attempt}.log').exists():attempt+=1
   log.rename(log.with_name(log.stem+f'-attempt-{attempt}.log'))
  with log.open('w') as out:r=subprocess.run([b[binary],group,'--test-threads=1',*[arg for name in excluded for arg in ['--skip',name]]],cwd=repo/'crates/codegen'/binary.replace('_','-'),env=env,stdout=out,stderr=subprocess.STDOUT,timeout=1200)
  matches=list(re.finditer(r'test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored',log.read_text()));m=matches[-1] if matches else None
  row=dict(binary=binary,binary_sha256=binary_sha,filter=group,log=log.name,exit=r.returncode,passed=int(m[2]) if m else 0,failed=int(m[3]) if m else None,ignored=int(m[4]) if m else None,release_debug_assert_only_exclusions=excluded);row['ok']=r.returncode==0 and row['passed']>0;results.append(row);(p/'checks.json').write_text(json.dumps(results,indent=2)+'\n');print(row,flush=True)
  if not row['ok']:raise SystemExit(1)
