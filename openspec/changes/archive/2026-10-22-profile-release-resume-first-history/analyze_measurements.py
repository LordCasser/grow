"""Reconstruct the median-total run without adding overlapping phase medians."""
import argparse
from datetime import datetime
import json
from pathlib import Path


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('output', type=Path)
    parser.add_argument('--stage', required=True)
    args=parser.parse_args()
    measurements=json.loads((args.output/f'{args.stage}-measurements.json').read_text())
    timelines={}
    for profile in dict.fromkeys(row['profile'] for row in measurements):
        rows=[row for row in measurements if row['profile']==profile]
        representative=sorted(rows,key=lambda row:row['load_ms'])[len(rows)//2]
        trace=args.output/representative['source'].replace('.txt','.trace.jsonl')
        start=None;intervals=[]
        for line in trace.read_text().splitlines():
            row=json.loads(line);fields=row.get('fields',row);event=fields.get('event')
            if event not in ('resume_load_start','timing','resume_first_history_received','resume_load_end'):
                continue
            stamp=datetime.fromisoformat(row['timestamp'].replace('Z','+00:00')).timestamp()
            if event=='resume_load_start':start=stamp;continue
            if start is None:continue
            offset=(stamp-start)*1000
            if event=='timing':
                duration=fields.get('elapsed_us',fields.get('elapsed_ms',0)*1000)/1000
                intervals.append({'name':fields['name'],'start_ms':offset-duration,'end_ms':offset,'duration_ms':duration})
            else:
                intervals.append({'name':event,'start_ms':offset,'end_ms':offset})
                if event=='resume_load_end':break
        assert start is not None and any(i['name']=='resume_first_history_received' for i in intervals),'trace markers missing'
        timelines[profile]={'source':trace.name,'load_ms':representative['load_ms'],
                            'first_history_ms':representative['first_history_ms'],
                            'intervals':sorted(intervals,key=lambda i:i['start_ms'])}
    (args.output/f'{args.stage}-timelines.json').write_text(json.dumps(timelines,indent=2)+'\n')
    print(f'{len(timelines)} representative timelines reconstructed')


if __name__=='__main__':main()
