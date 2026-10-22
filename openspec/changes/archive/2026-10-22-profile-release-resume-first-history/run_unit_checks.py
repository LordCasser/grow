"""Run built scoped test targets, rejecting empty or ignored-only selections."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess

FILTERS = {
    'shell': ['session::storage::jsonl::', 'session::persistence::',
              'session::response_projection::tests', 'session::storage::tests',
              'agent::mvp_agent::tests', 'session::storage::communication_history::',
              'session::storage::transcript::'],
    'pager': ['loading_replay_ignores_input_and_preserves_existing_draft_until_session_loaded',
              'loading_replay_keeps_confirmed_ctrl_d_and_resize_available',
              'app::acp_handler::tests::reconnect::'],
    'chat-state': ['timeline::'],
}


def main():
    p=argparse.ArgumentParser()
    p.add_argument('binary', type=Path)
    p.add_argument('--kind', choices=FILTERS, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--stage', required=True)
    args=p.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    env=os.environ.copy();env['RUST_MIN_STACK']='16777216'
    rows=[]
    for f in FILTERS[args.kind]:
        label=f.replace('::','-').strip('-')
        log=args.output/f'{args.stage}-unit-{label}.txt'
        with log.open('w') as file:
            r=subprocess.run([str(args.binary.resolve()), f, '--test-threads=1', '--nocapture'],
                             env=env,stdout=file,stderr=subprocess.STDOUT,timeout=600)
        m=re.search(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored',log.read_text())
        if r.returncode or not m or int(m[1])==0:
            raise SystemExit(f'{f}: failed or empty; see {log}')
        row={'filter':f,'passed':int(m[1]),'ignored':int(m[3]),'source':log.name}
        rows.append(row);print(row,flush=True)
        (args.output/f'{args.stage}-{args.kind}-unit-results.json').write_text(json.dumps(rows,indent=2)+'\n')


if __name__=='__main__':main()
