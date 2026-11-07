import json,sys,glob,os
sid=sys.argv[1]
fs=glob.glob(os.path.expanduser(f"~/.claude/projects/*/{sid}*.jsonl"))
for f in fs:
    print("##", f.replace(os.path.expanduser("~"),"~"))
    for l in open(f):
        d=json.loads(l); m=d.get('message')
        c=m.get('content') if isinstance(m,dict) else None
        if d.get('type') in ('agent-setting','custom-title','permission-mode','worktree-state') or 'worktree' in d.get('type',''): print(d.get('type'), {k:v for k,v in d.items() if k not in ('sessionId',)})
        if isinstance(c,list):
            for x in c:
                if x.get('type')=='text': print(d['type'],'TEXT',x['text'][:300])
                elif x.get('type')=='tool_use': print(d['type'],'TOOL_USE',x['name'],json.dumps(x.get('input'))[:200])
                elif x.get('type')=='tool_result': print(d['type'],'TOOL_RESULT',str(x.get('content'))[:400])
        elif isinstance(c,str) and d.get('type')=='user': print('user STR',c[:200])
