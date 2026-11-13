import {AmbientClock} from './activity.js';
import {AgentBrain,agentHitBox} from './brain.js';
import {SpriteAtlas,PuppetRenderer} from './puppet.js';
import {ROLES,OfficeModel,Camera,stationHome,MAX_AGENTS,MAX_DESKS} from './office-model.js';
import {RoomRenderer,ACTOR_SCALE} from './room.js';
const $=s=>document.querySelector(s),workspace=$('#workspace'),canvas=$('#world'),ctx=canvas.getContext('2d');
const storageKey='agent-hq-modular-v1';let saved=null;try{saved=JSON.parse(localStorage.getItem(storageKey));}catch{}
let model=new OfficeModel(saved),brains=new Map(),targets=new Map(),selected=null,hovered=null,ready=false,painter=null,faces=null;
const camera=new Camera(),clock=new AmbientClock(),reduced=matchMedia('(prefers-reduced-motion: reduce)');
let running=!reduced.matches,manualMotion=false,raf=0,lastPaint=-Infinity,lastUI=-Infinity,viewWidth=1,viewHeight=1,dpr=1,toastTimeout;
const pointers=new Map();let gesture=null;
function persist(){try{localStorage.setItem(storageKey,JSON.stringify(model.serialize()));}catch{toast('Your layout works here, but browser storage is unavailable.');}}
function toast(message){$('#toast').textContent=message;$('#toast').classList.add('visible');clearTimeout(toastTimeout);toastTimeout=setTimeout(()=>$('#toast').classList.remove('visible'),3300);}
function portrait(c,role){if(!faces)return;const image=faces.parts[ROLES[role].index*2],q=c.getContext('2d');q.clearRect(0,0,c.width,c.height);const scale=Math.min(c.width/image.width,c.height/image.height)*.94;q.imageSmoothingQuality='high';q.drawImage(image,(c.width-image.width*scale)/2,(c.height-image.height*scale)/2,image.width*scale,image.height*scale);}
function sync(){
 const alive=new Set(model.agents.map(a=>a.id));for(const id of brains.keys())if(!alive.has(id))brains.delete(id);
 for(const [i,a] of model.agents.entries())if(!brains.has(a.id))brains.set(a.id,new AgentBrain({...stationHome(model.deskFor(a).index),role:a.role,name:a.name,seed:i}));
 $('#agent-targets').replaceChildren();targets.clear();
 for(const a of model.agents){const b=document.createElement('button');b.className='agent-button';b.setAttribute('aria-label',`${a.name}, ${ROLES[a.role].title}. Say hello and interact.`);b.setAttribute('aria-controls','inspector');b.setAttribute('aria-expanded',String(selected===a.id));b.addEventListener('click',event=>{if(event.detail===0)select(a.id,true);});b.addEventListener('focus',()=>{hovered=a.id;draw();});b.addEventListener('blur',()=>{hovered=null;draw();});$('#agent-targets').append(b);targets.set(a.id,b);}
 renderCrew();updateCounts();
}
function renderCrew(){
 $('#crew').replaceChildren();
 for(const a of model.agents){const role=ROLES[a.role],button=document.createElement('button');button.className='crew-card';button.dataset.id=a.id;button.style.setProperty('--role-color',role.color);button.setAttribute('aria-label',`${a.name}, ${role.title}`);button.setAttribute('aria-pressed',String(selected===a.id));
 const title=document.createElement('strong');title.textContent=role.title.toUpperCase();const head=document.createElement('canvas');head.width=150;head.height=140;
 const info=document.createElement('div');info.className='mini-info';const name=document.createElement('span');name.textContent=a.name;const state=document.createElement('span');state.className='mini-state';state.textContent='ACTIVE';info.append(name,state);const bar=document.createElement('div');bar.className='mini-bar';bar.append(document.createElement('i'));button.append(title,head,info,bar);button.addEventListener('click',()=>select(a.id,true));$('#crew').append(button);portrait(head,a.role);}
}
function updateCounts(){
 $('#agent-count').textContent=model.agents.length+' '+(model.agents.length===1?'agent':'agents');
 $('#desk-count').textContent=model.desks.length+' workspaces'+(model.emptyDesks?' · '+model.emptyDesks+' free':'');
 $('#space-label').textContent=model.desks.length+' WORKSPACES';$('#add-agent').disabled=!ready||model.agents.length>=MAX_AGENTS;$('#add-desk').disabled=!ready||model.desks.length>=MAX_DESKS;
}
function screenBounds(brain){const b=agentHitBox(brain),p=camera.screen({x:brain.x+(b.x-brain.x)*ACTOR_SCALE,y:brain.y+(b.y-brain.y)*ACTOR_SCALE});return {x:p.x,y:p.y,width:b.width*ACTOR_SCALE*camera.scale,height:b.height*ACTOR_SCALE*camera.scale};}
function updateTargets(){
 for(const [id,b] of targets){const brain=brains.get(id);if(!brain)continue;const r=screenBounds(brain);b.hidden=r.x+r.width<0||r.x>viewWidth||r.y+r.height<0||r.y>viewHeight;b.style.left=r.x+'px';b.style.top=r.y+'px';b.style.width=r.width+'px';b.style.height=r.height+'px';}
 const brain=brains.get(selected||hovered),a=model.agents.find(a=>a.id===(selected||hovered));
 if(brain&&a){const p=camera.screen({x:brain.x,y:brain.y-221*ACTOR_SCALE});const speaking=brain.time<brain.bubbleUntil;$('#speech').textContent=speaking?brain.bubble:a.name+' · '+brain.status;$('#speech').style.left=Math.max(90,Math.min(viewWidth-90,p.x))+'px';$('#speech').style.top=Math.max(55,p.y)+'px';$('#speech').classList.toggle('visible',p.x>-20&&p.x<viewWidth+20&&p.y>-30&&p.y<viewHeight-110);}else $('#speech').classList.remove('visible');
}
function draw(){if(!ready)return;painter.draw(ctx,model,brains,camera,{time:clock.elapsed,selected,hovered,width:viewWidth,height:viewHeight,dpr});updateTargets();$('#zoom-label').textContent=Math.round(camera.scale/camera.fitScale*100)+'%';}
function updateUI(){
 for(const card of $('#crew').children){const id=card.dataset.id,b=brains.get(id);if(!b)continue;card.classList.toggle('selected',id===selected);card.setAttribute('aria-pressed',String(id===selected));card.querySelector('.mini-bar i').style.width=Math.max(8,b.progress*100)+'%';card.querySelector('.mini-state').textContent=running?b.state.kind.toUpperCase():'PAUSED';}
 const a=model.agents.find(a=>a.id===selected),brain=brains.get(selected);if(!a||!brain)return;
 $('#agent-role').textContent=ROLES[a.role].title.toUpperCase();$('#agent-title').textContent=a.name;$('#agent-description').textContent=ROLES[a.role].description;$('#work-label').textContent=ROLES[a.role].work;$('#agent-state').textContent=running?brain.status:'Animation paused';$('#agent-progress').style.width=brain.progress*100+'%';$('.progress').setAttribute('aria-valuenow',String(Math.round(brain.progress*100)));$('#completed').textContent=brain.completed+' tasks completed';
 const history=brain.history.length?brain.history:[{at:0,text:'Settled in at the workspace'}];const fingerprint=selected+JSON.stringify(history),log=$('#activity-log');if(log.dataset.history!==fingerprint){log.replaceChildren(...history.slice(0,3).map(entry=>{const li=document.createElement('li'),time=document.createElement('time'),text=document.createElement('span');time.textContent=String(Math.floor(entry.at/60)).padStart(2,'0')+':'+String(Math.floor(entry.at%60)).padStart(2,'0');text.textContent=entry.text;li.append(time,text);return li;}));log.dataset.history=fingerprint;}
}
function select(id,focus=false){
 if(!ready||!brains.has(id))return;selected=id;$('#inspector').hidden=false;$('#feedback').textContent='';for(const [agentId,target] of targets)target.setAttribute('aria-expanded',String(agentId===id));
 const brain=brains.get(id);if(running)brain.request('hello');else $('#feedback').textContent='Resume animation to see a response.';
 if(focus||viewWidth<760||camera.screen(brain).x>viewWidth-310){camera.scale=Math.max(camera.scale,viewWidth<760?1:1.05);camera.x=(viewWidth<760?viewWidth*.5:viewWidth*.43)-brain.x*camera.scale;camera.y=(viewWidth<760?viewHeight*.47:viewHeight*.61)-brain.y*camera.scale;}
 updateUI();draw();
}
function closeInspector(){selected=null;$('#inspector').hidden=true;for(const b of targets.values())b.setAttribute('aria-expanded','false');updateUI();draw();}
function interact(action){const b=brains.get(selected);if(!b)return;if(!running){$('#feedback').textContent='Resume animation to see a response.';return;}b.request(action);$('#feedback').textContent=action==='hello'?'A little hello, just for you.':action==='blueprint'?'Watch the idea take shape.':'A few steps, then a good stretch.';updateUI();draw();}
function fit(){camera.fit(model.room,viewWidth,viewHeight);draw();}
function resize(){const r=workspace.getBoundingClientRect();viewWidth=Math.max(1,r.width);viewHeight=Math.max(1,r.height);dpr=Math.min(devicePixelRatio||1,2);canvas.width=Math.round(viewWidth*dpr);canvas.height=Math.round(viewHeight*dpr);camera.fit(model.room,viewWidth,viewHeight);draw();}
function frame(now){raf=0;if(!running||!ready||document.hidden){clock.suspend();return;}const before=clock.elapsed;clock.tick(now);const dt=clock.elapsed-before;for(const b of brains.values())b.update(dt);if(now-lastPaint>=1000/30-1){draw();lastPaint=now;}if(now-lastUI>=350){updateUI();lastUI=now;}raf=requestAnimationFrame(frame);}
function schedule(){if(ready&&running&&!document.hidden&&!raf)raf=requestAnimationFrame(frame);}
function setRunning(value){running=value;if(raf)cancelAnimationFrame(raf);raf=0;clock.suspend();$('#motion').textContent=running?'Ⅱ':'▶';$('#motion').setAttribute('aria-label',running?'Pause animation':'Resume animation');$('#motion').setAttribute('aria-pressed',String(!running));$('#scene-mode').textContent=running?'AUTONOMOUS OFFICE':'ANIMATION PAUSED';updateUI();schedule();}
function localPoint(event){const r=workspace.getBoundingClientRect();return {x:event.clientX-r.left,y:event.clientY-r.top};}
function hit(point){return [...model.agents].sort((a,b)=>brains.get(b.id).y-brains.get(a.id).y).find(a=>{const r=screenBounds(brains.get(a.id));return point.x>=r.x&&point.x<=r.x+r.width&&point.y>=r.y&&point.y<=r.y+r.height;})?.id||null;}
workspace.addEventListener('pointerdown',event=>{
 if(!ready||event.target.closest('.inspector,.builder,.view-controls')||(event.button!==0&&event.pointerType==='mouse'))return;
 const p=localPoint(event);pointers.set(event.pointerId,p);workspace.setPointerCapture(event.pointerId);
 if(pointers.size===1)gesture={start:p,last:p,moved:false};
 if(pointers.size===2){const [a,b]=[...pointers.values()];gesture={moved:true,distance:Math.hypot(a.x-b.x,a.y-b.y),mid:{x:(a.x+b.x)/2,y:(a.y+b.y)/2}};}
});
workspace.addEventListener('pointermove',event=>{
 const p=localPoint(event);
 if(pointers.has(event.pointerId)){
  pointers.set(event.pointerId,p);
  if(pointers.size===2){const [a,b]=[...pointers.values()],distance=Math.hypot(a.x-b.x,a.y-b.y),mid={x:(a.x+b.x)/2,y:(a.y+b.y)/2};if(gesture.distance>0)camera.zoom(distance/gesture.distance,mid);camera.pan(mid.x-gesture.mid.x,mid.y-gesture.mid.y);gesture.distance=distance;gesture.mid=mid;gesture.moved=true;}
  else if(gesture.start){if(Math.hypot(p.x-gesture.start.x,p.y-gesture.start.y)>4)gesture.moved=true;if(gesture.moved)camera.pan(p.x-gesture.last.x,p.y-gesture.last.y);gesture.last=p;}
  canvas.classList.toggle('dragging',!!gesture.moved);draw();return;
 }
 if(!ready)return;const next=hit(p);if(hovered!==next){if(brains.has(hovered))brains.get(hovered).setGaze(brains.get(hovered).x);hovered=next;draw();}if(hovered)brains.get(hovered).setGaze(camera.world(p).x);
});
function endPointer(event,cancel=false){if(!pointers.has(event.pointerId))return;const click=!cancel&&pointers.size===1&&!gesture.moved,p=localPoint(event);pointers.delete(event.pointerId);if(click){const id=hit(p);if(id)select(id);else closeInspector();}if(pointers.size===1){const remaining=[...pointers.values()][0];gesture={start:remaining,last:remaining,moved:true};}if(!pointers.size){gesture=null;canvas.classList.remove('dragging');} }
workspace.addEventListener('pointerup',event=>endPointer(event));workspace.addEventListener('pointercancel',event=>endPointer(event,true));
workspace.addEventListener('pointerleave',()=>{if(!pointers.size){hovered=null;draw();}});
workspace.addEventListener('wheel',event=>{if(!ready||event.target.closest('.inspector'))return;event.preventDefault();camera.zoom(Math.exp(-event.deltaY*.0012),localPoint(event));draw();},{passive:false});
$('#zoomin').onclick=()=>{camera.zoom(1.2);draw();};$('#zoomout').onclick=()=>{camera.zoom(1/1.2);draw();};$('#fit').onclick=fit;$('#motion').onclick=()=>{manualMotion=true;setRunning(!running);};$('#close-inspector').onclick=closeInspector;
for(const b of document.querySelectorAll('[data-action]'))b.onclick=()=>interact(b.dataset.action);
$('#add-desk').onclick=()=>{const desk=model.addDesk();if(!desk){toast('All 48 workspaces are in place.');return;}persist();sync();closeInspector();fit();toast('Workspace '+(desk.index+1)+' added · '+model.emptyDesks+' available');};
$('#add-agent').onclick=()=>{$('#add-explanation').textContent=model.emptyDesks?`${model.emptyDesks} workspace${model.emptyDesks===1?' is':'s are'} ready for a new teammate.`:'Choose a role. A new workspace will be added automatically.';$('#add-dialog').showModal();};
function addAgent(role){const before=model.desks.length,agent=model.addAgent(role);if(!agent){toast('The studio is at its agent capacity.');return;}persist();sync();$('#add-dialog').close();fit();select(agent.id,viewWidth<760);toast(agent.name+' joined'+(model.desks.length>before?' · the office has grown':' · workspace ready'));const card=[...$('#crew').children].find(c=>c.dataset.id===agent.id);card?.scrollIntoView({behavior:reduced.matches?'instant':'smooth',block:'nearest',inline:'nearest'});}
$('#remove-agent').onclick=()=>{const agent=model.agents.find(a=>a.id===selected);if(!agent)return;model.removeAgent(agent.id);persist();closeInspector();sync();draw();toast(agent.name+' left · the workspace is available');};
$('#about').onclick=()=>$('#about-dialog').showModal();for(const b of document.querySelectorAll('[data-close]'))b.onclick=()=>b.closest('dialog').close();
for(const dialog of document.querySelectorAll('dialog'))dialog.addEventListener('click',event=>{const r=dialog.getBoundingClientRect();if(event.target===dialog&&(event.clientX<r.left||event.clientX>r.right||event.clientY<r.top||event.clientY>r.bottom))dialog.close();});
$('#reset').onclick=()=>{if(!confirm('Reset this browser’s studio to the original six agents and workspaces?'))return;model=new OfficeModel();brains.clear();selected=null;hovered=null;persist();sync();$('#about-dialog').close();closeInspector();fit();toast('The original crew is back.');};
reduced.addEventListener('change',event=>{if(!manualMotion)setRunning(!event.matches);});
document.addEventListener('visibilitychange',()=>{if(document.hidden){if(raf)cancelAnimationFrame(raf);raf=0;clock.suspend();}else schedule();});window.addEventListener('pagehide',()=>{if(raf)cancelAnimationFrame(raf);raf=0;clock.suspend();});window.addEventListener('pageshow',schedule);window.addEventListener('blur',()=>{pointers.clear();gesture=null;canvas.classList.remove('dragging');});
document.addEventListener('keydown',event=>{if(document.querySelector('dialog[open]'))return;if(event.key==='Escape')closeInspector();if(event.key.toLowerCase()==='f')fit();});
new ResizeObserver(resize).observe(workspace);window.addEventListener('resize',resize);sync();setRunning(running);
async function initialize(){
 try{
  const load=async name=>{const image=new Image();image.src='assets/'+name;await image.decode();return image;};
  const [base,roleImages,props,floorImage]=await Promise.all(['architect-parts.png','roles.png','furniture.png','floor.png'].map(load));
  const limbs=new SpriteAtlas(base);faces=new SpriteAtlas(roleImages);const furniture=new SpriteAtlas(props,undefined,3,3,true),rigs={};
  for(const [id,r] of Object.entries(ROLES))rigs[id]=new PuppetRenderer(limbs,faces.parts[r.index*2],faces.parts[r.index*2+1]);
  const floor=document.createElement('canvas');floor.width=256;floor.height=256;floor.getContext('2d').drawImage(floorImage,0,0,256,256);
  painter=new RoomRenderer({floor,furniture,rigs});ready=true;renderCrew();
  for(const [id,role] of Object.entries(ROLES)){const b=document.createElement('button');b.className='role-choice';const c=document.createElement('canvas');c.width=140;c.height=140;const title=document.createElement('strong');title.textContent=role.title;const small=document.createElement('small');small.textContent=role.name+' & friends';b.append(c,title,small);b.onclick=()=>addAgent(id);$('#role-options').append(b);portrait(c,id);}
  $('#loading').hidden=true;updateCounts();resize();schedule();
 }catch(error){console.error('Studio initialization failed.',error);$('#loading').replaceChildren();const label=document.createElement('strong');label.textContent='The studio couldn’t load.';const retry=document.createElement('button');retry.textContent='Try again';retry.onclick=()=>location.reload();$('#loading').append(label,retry);}
}
initialize();
