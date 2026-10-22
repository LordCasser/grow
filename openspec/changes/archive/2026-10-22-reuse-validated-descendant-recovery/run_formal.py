"""Serialize measured groups after builds, regressions and warmups finish."""
from pathlib import Path
import subprocess,sys
p=Path(__file__).resolve().parent;repo=next(root for root in p.parents if (root/'Cargo.toml').is_file() and (root/'openspec').is_dir());bins=Path('target/descendant-recovery-release/binaries').resolve();pty=repo/'openspec/changes/archive/2026-10-22-defer-historical-markdown-presentation/run_pty_pairs.py';base=[sys.executable,str(pty),str(bins/'after-pty_e2e_persistence'),str(bins/'before-grow'),str(bins/'after-grow'),'--runs','3','--turns','128,512,2048','--slow-turns','512,2048']
commands=[base+['--output',str(p/'pty-default-off'),'--instrumentation-off'],base+['--output',str(p/'pty-log')],[sys.executable,str(p/'run_descendant_pairs.py'),str(bins/'before-pager'),str(bins/'after-pager'),'--output',str(p/'descendant-pairs')],[sys.executable,str(p/'run_rpc_pairs.py')]]
for command in commands:
 print('start',command[-1],flush=True);subprocess.run(command,check=True)
