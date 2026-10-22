"""Diagnostic RSS sampling after fixture generation; timings are not A/B data."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import threading
import time


def run(binary, output, stage, repeat):
    label=f'{stage}-rss-2048-{repeat}'
    log=output/f'{label}.txt'
    env={k:v for k,v in os.environ.items() if not k.startswith('GROW_')}
    env.update(RUST_MIN_STACK='16777216',GROW_PERF_SESSION_FORMAT='modern',
               GROW_PERF_TURNS='2048',GROW_PERF_AGENT_CHUNKS_PER_TURN='8',
               GROW_PERF_AGENT_CHUNK_LEN='4096')
    process=subprocess.Popen([str(binary.resolve()),'full_session_load_e2e','--exact','--ignored','--nocapture','--test-threads=1'],env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True,bufsize=1)
    stop=threading.Event();samples=[];worker=None
    def sample():
        while not stop.is_set():
            result=subprocess.run(['/bin/ps','-o','rss=','-p',str(process.pid)],capture_output=True,text=True)
            value=result.stdout.strip()
            if value.isdigit():samples.append({'at':time.monotonic(),'bytes':int(value)*1024})
            stop.wait(0.025)
    with log.open('w') as file:
        for line in process.stdout:
            file.write(line)
            if worker is None and '[perf] e2e session:' in line:
                worker=threading.Thread(target=sample);worker.start()
    code=process.wait();stop.set()
    if worker:worker.join()
    if code or not samples:raise SystemExit(f'{label}: failed or no samples')
    row={'stage':stage,'run':repeat,'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),
         'scope':'fixture-complete marker through exit; includes agent initialization/cleanup; ps observer overhead; timings excluded',
         'first_rss_bytes':samples[0]['bytes'],'peak_rss_bytes':max(s['bytes'] for s in samples),
         'samples':len(samples),'source':log.name}
    print(row,flush=True)
    return row


def main():
    p=argparse.ArgumentParser();p.add_argument('before',type=Path);p.add_argument('after',type=Path);p.add_argument('--output',type=Path,required=True)
    args=p.parse_args();rows=[]
    for repeat in range(1,3):
        for stage,binary in [('before',args.before),('after',args.after)]:
            rows.append(run(binary,args.output,stage,repeat))
            (args.output/'rss-diagnostic.json').write_text(json.dumps(rows,indent=2)+'\n')


if __name__=='__main__':main()
