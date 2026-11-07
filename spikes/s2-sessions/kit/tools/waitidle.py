import json,subprocess,sys,time
sid=sys.argv[1]; deadline=time.time()+float(sys.argv[2] if len(sys.argv)>2 else 60); seen=[]
while time.time()<deadline:
    d=json.loads(subprocess.run(["/tmp/ac-spike-s2/tools/cc","agents","--json","--all"],capture_output=True,text=True,stdin=subprocess.DEVNULL).stdout)
    m=[s for s in d if s.get("id")==sid or s.get("sessionId","").startswith(sid)]
    st=tuple((s.get("status"),s.get("state")) for s in m)
    if not seen or seen[-1][1]!=st: seen.append((round(time.time(),1),st)); print(seen[-1], json.dumps(m))
    if m and all(s.get("status")=="idle" for s in m) and len(seen)>0 and time.time()>deadline-1e9: 
        if len(sys.argv)>3 and sys.argv[3]=="once": break
        if m[0].get("state") in ("done","blocked","failed","stopped"): break
    time.sleep(1)
