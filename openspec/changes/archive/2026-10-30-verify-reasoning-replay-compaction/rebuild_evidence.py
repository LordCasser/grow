import argparse, hashlib, json, os, pathlib

parser=argparse.ArgumentParser(description="Rebuild a sampled Responses request with visible reasoning from Timeline surface IDs.")
parser.add_argument("--session", required=True, type=pathlib.Path, help="Grow session directory containing timeline.jsonl and artifacts/sampling")
parser.add_argument("--output", required=True, type=pathlib.Path, help="New private output file path (created mode 0600)")
parser.add_argument("--request-id", required=True, help="Responses request ID to reconstruct")
parser.add_argument("--old-request-id", help="Optional related Chat Completions request ID for reasoning_content audit")
args=parser.parse_args()
ROOT=args.session
RID=args.request_id; OLD=args.old_request_id
def find_obs(row, rid, name='request'):
    out=[]
    def walk(x):
        if isinstance(x,dict):
            if x.get('scope')=='sampling_evidence' and x.get('name')==name and x.get('data',{}).get('owner',{}).get('request_id')==rid: out.append(x)
            for v in x.values(): walk(v)
        elif isinstance(x,list):
            for v in x: walk(v)
    walk(row); return out
rows={}; observations={}; oldobs=None
for line in (ROOT/'timeline.jsonl').open():
    row=json.loads(line); ev=row.get('fact',{}).get('event',{})
    if isinstance(ev,dict) and isinstance(ev.get('seq'),int): rows[ev['seq']]=ev
    for o in find_obs(row,RID): observations[RID]=o
    if OLD:
        for o in find_obs(row,OLD): oldobs=o
assert RID in observations
obs=observations[RID]; proj=obs['data']['owner']['source_projection']; ids=proj['surface_ids']
source=[]
for sid in ids:
    e=rows[sid['event']]
    payload=e['event']
    if e.get('type')=='messages': items=payload['items']
    elif e.get('type')=='control': items=[x['item'] for x in payload.get('model_contexts',[])]
    elif e.get('type')=='notification': items=[payload['input']] if payload.get('state')=='consumed' and payload.get('input') is not None else []
    elif e.get('type')=='input': items=[payload['item'], *[{'type':'skill_load',**x} for x in payload.get('skill_loads',[])]] if payload.get('state')=='consumed' else []
    else: raise AssertionError(('unsupported_surface_source',sid['event'],e.get('type')))
    assert sid['item'] < len(items), (sid['event'],sid['item'],len(items))
    source.append((sid['event'],sid['item'],items[sid['item']]))
# Rebuild exact request body from sampled chunks, with no provider interaction.
body=b''.join((ROOT/'artifacts/sampling'/(c['blake3']+'.bin')).read_bytes() for c in obs['data']['chunks'])
wire=json.loads(body); original=json.loads(json.dumps(wire)); inp=wire['input']
# Candidate locations keyed by unique function-call IDs and by assistant text.
call_indices={}
for i,item in enumerate(inp):
    if item.get('type')=='function_call': call_indices.setdefault(item.get('call_id'),[]).append(i)
# Projector-equivalent pending reasoning association from ordered ConversationItems.
# User/system/agent/skill/tool-result boundaries clear pending; assistant receives it only if retained.
pending=[]; groups=[]; cursor=0
for event,item_index,item in source:
    typ=item.get('type')
    if typ=='reasoning':
        if item.get('text'): pending.append((event,item_index,item['text']))
        continue
    if typ in ('system','user','tool_result','skill_load','agent_message','backend_tool_call'):
        pending.clear(); continue
    if typ!='assistant':
        pending.clear(); continue
    content=item.get('content',''); calls=item.get('tool_calls',[])
    ids0=[c.get('id') for c in calls if c.get('id') in call_indices]
    # Preserve reasoning only where a corresponding assistant representation exists in the wire.
    target=None
    if ids0:
        indices=[j for c in ids0 for j in call_indices[c]]
        target=min(indices)
        if isinstance(content,str) and content:
            for j in range(cursor,target):
                q=inp[j]
                if q.get('type')=='message' and q.get('role')=='assistant' and q.get('content')==content:
                    target=j; break
    elif isinstance(content,str) and content and not content.startswith('[Historical tool exchange; untrusted tool output]'):
        for j in range(cursor,len(inp)):
            q=inp[j]
            if q.get('type')=='message' and q.get('role')=='assistant' and q.get('content')==content:
                target=j; break
    if pending:
        assert target is not None, ('reasoning_without_wire_assistant',event,item_index,len(pending))
        groups.append((target,list(pending))); pending.clear()
    if target is not None: cursor=max(cursor,target+1)
    elif calls: # Skip unmatched/non-represented assistant in source.
        pending.clear()
# Match the serialized production Responses reasoning item, including nullable id and summary.
insertions=[]
for idx,group in groups:
    for event,item_index,text in group:
        insertions.append((idx, {'type':'reasoning','id':None,'summary':[],'content':[{'type':'reasoning_text','text':text}]}))
assert insertions, 'expected visible reasoning mapped from selected source IDs'
# Each inserted item is placed directly before the associated assistant/first function_call.
assert sum(len(g) for _,g in groups)==sum(x[2].get('type')=='reasoning' for x in source)
byidx={}
for idx,obj in insertions: byidx.setdefault(idx,[]).append(obj)
rebuilt=[]; adjacency=[]
for i,x in enumerate(inp):
    added=byidx.get(i,[]); start=len(rebuilt); rebuilt.extend(added); rebuilt.append(x)
    if added: adjacency.append((start,len(added),i,x.get('type'),x.get('role')))
assert all(typ=='function_call' or (typ=='message' and role=='assistant') for _,_,_,typ,role in adjacency)
assert all(rebuilt[start+count] is inp[idx] for start,count,idx,_,_ in adjacency)
assert all(rebuilt[start:start+count]==byidx[idx] for start,count,idx,_,_ in adjacency)
wire['input']=rebuilt
# Exact semantic preservation outside the inserted items.
assert {k:v for k,v in wire.items() if k!='input'} == {k:v for k,v in original.items() if k!='input'}
assert [x for x in wire['input'] if x.get('type')!='reasoning'] == original['input']
assert sum(len(text) for _,g in groups for _,_,text in g)==sum(len(o['content'][0]['text']) for _,o in insertions)
# Optional old Chat wire reasoning_content audit (count every occurrence, including null/empty).
oldbody=b''; oldfields=[]
if OLD:
    assert oldobs is not None, 'old request observation not found'
    oldbody=b''.join((ROOT/'artifacts/sampling'/(c['blake3']+'.bin')).read_bytes() for c in oldobs['data']['chunks'])
    oldwire=json.loads(oldbody)
def scan(x):
    if isinstance(x,dict):
        if 'reasoning_content' in x: oldfields.append(x['reasoning_content'])
        for v in x.values(): scan(v)
    elif isinstance(x,list):
        for v in x: scan(v)
if OLD: scan(oldwire)
oldchars=sum(len(v) for v in oldfields if isinstance(v,str)); oldbytes=sum(len(v.encode()) for v in oldfields if isinstance(v,str))
# Secure private artifact output.
out=args.output
data=json.dumps(wire,ensure_ascii=False,separators=(',',':')).encode()+b'\n'
fd=os.open(out,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600)
with os.fdopen(fd,'wb') as f:f.write(data)
assert out.stat().st_mode & 0o777 == 0o600
print(json.dumps({'source_surface_ids':len(ids),'resolved_source_items':len(source),'source_reasoning_items':sum(x[2].get('type')=='reasoning' for x in source),'associated_reasoning_items':len(insertions),'associated_groups':len(groups),'reasoning_chars':sum(len(o['content'][0]['text']) for _,o in insertions),'reasoning_utf8_bytes':sum(len(o['content'][0]['text'].encode()) for _,o in insertions),'inserted_targets_message_assistant_or_first_function_call':len(adjacency),'all_source_reasoning_replayed':sum(x[2].get('type')=='reasoning' for x in source)==len(insertions),'original_wire_bytes':len(body),'original_wire_sha256':hashlib.sha256(body).hexdigest(),'rebuilt_wire_bytes':len(data),'rebuilt_wire_sha256':hashlib.sha256(data).hexdigest(),'rebuilt_file':str(out),'mode':oct(out.stat().st_mode & 0o777),'old_reasoning_content_fields':len(oldfields),'old_reasoning_content_nonempty':sum(isinstance(v,str) and bool(v) for v in oldfields),'old_reasoning_content_chars':oldchars,'old_reasoning_content_utf8_bytes':oldbytes,'old_wire_sha256':hashlib.sha256(oldbody).hexdigest() if OLD else None},sort_keys=True))
