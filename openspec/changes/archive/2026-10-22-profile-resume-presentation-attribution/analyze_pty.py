"""Read retained trace evidence; never add nested phases or mix clock origins."""
import argparse,datetime,json,re,statistics
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('directory',type=Path);a=p.parse_args()
def fields(row):
 f=row.get('fields',row).copy()
 # Existing InstrumentationTimer serializes its numeric metadata as Rust Debug.
 for k,n,b in re.findall(r'"(\w+)": (?:Number\((\d+)\)|Bool\((true|false)\))',f.get('fields','')):
  f[k]=int(n) if n else b=='true'
 return f
def timestamp(row):return datetime.datetime.fromisoformat(row['timestamp'].replace('Z','+00:00'))
rows=[]
for path in sorted(a.directory.glob('*/*-loading.json')):
 t=json.loads(path.read_text());parts=path.parent.name.rsplit('-',2);stage,case,repeat=parts
 loading=t['loading']['instrumentation_prefix_through_completion'];full=[json.loads(l) for l in (path.parent/t['raw_files']['instrumentation']).read_text().splitlines()]
 phase={}
 for row in loading:
  f=fields(row)
  if f.get('event')!='timing':continue
  name=f['name'];out=phase.setdefault(name,{'count':0,'sum_ms':0.,'max_ms':0.,'source_bytes':0})
  out['count']+=1;out['sum_ms']+=f['elapsed_us']/1000;out['max_ms']=max(out['max_ms'],f['elapsed_us']/1000);out['source_bytes']+=f.get('source_bytes',0)
 item={'stage':stage,'case':case,'repeat':int(repeat),'turns':t['turns'],'source':str(path.relative_to(a.directory)),'source_stats':t['source'],'phases':phase,'slow_progress':t['slow_replay_progress'],'events':t['loading']['events']}
 batch=[fields(r) for r in loading if fields(r).get('name')=='pager.acp.apply_batch' and fields(r).get('loading')]
 item['loading_applied_notifications']=sum(f.get('count',0) for f in batch)
 item['loading_apply_batch_ms']=sum(f['elapsed_us']/1000 for f in batch)
 loading_seqs=set()
 for r in full:
  f=fields(r)
  if f.get('name')=='pager.frame.present' and f.get('loading'):
   loading_seqs.update(range(f['queued_before']+1,f['queued_after']+1))
 item['loading_frame_sequences']=sorted(loading_seqs)
 waits=[fields(r)['elapsed_us']/1000 for r in full if fields(r).get('name')=='pager.frame.wait' and fields(r).get('sequence') in loading_seqs]
 item['loading_frame_wait_ms']=waits
 deliveries={fields(r)['sequence']:timestamp(r) for r in full if fields(r).get('name')=='pager.frame.delivery' and fields(r).get('success')}
 acks=[(timestamp(r)-deliveries[f['sequence']]).total_seconds()*1000 for r in full if (f:=fields(r)).get('event')=='frame_ack' and f['sequence'] in deliveries and f['sequence'] in loading_seqs]
 item['loading_writer_ack_observation_delta_ms']=acks
 rows.append(item)
(a.directory/'attribution.json').write_text(json.dumps(rows,indent=2)+'\n')
summary=[]
for key in sorted({(r['stage'],r['case'],r['turns']) for r in rows}):
 selected=[r for r in rows if (r['stage'],r['case'],r['turns'])==key];s={'stage':key[0],'case':key[1],'turns':key[2],'runs':len(selected),'events':{},'phases':{}}
 for event in sorted({e['kind'] for r in selected for e in r['events']}):
  vals=[e['elapsed_ms'] for r in selected for e in r['events'] if e['kind']==event];s['events'][event]={'median_ms':statistics.median(vals),'range_ms':[min(vals),max(vals)]}
 for phase in sorted({p for r in selected for p in r['phases']}):
  vals=[r['phases'].get(phase,{}).get('sum_ms',0) for r in selected];counts=[r['phases'].get(phase,{}).get('count',0) for r in selected];s['phases'][phase]={'median_ms':statistics.median(vals),'range_ms':[min(vals),max(vals)],'counts':counts}
 summary.append(s)
(a.directory/'attribution-summary.json').write_text(json.dumps(summary,indent=2)+'\n')
for s in summary:
 print(s['stage'],s['case'],s['turns'], 'last',s['events'].get('last_history_marker_visible'), 'finish',s['phases'].get('pager.markdown.finish'),'wrap',s['phases'].get('pager.markdown.wrap'))
