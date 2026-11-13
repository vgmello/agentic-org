export const ROLES=Object.freeze({
 director:{title:'The Director',name:'Atlas',color:'#dcc373',index:0,prop:5,description:'Keeping the big picture in view, one thoughtful plan at a time.',work:'Plan the next milestone'},
 architect:{title:'The Architect',name:'Nova',color:'#84dbec',index:1,prop:1,description:'A curious mind, a few big ideas, and a whiteboard to work them out.',work:'Review the blueprint'},
 lead:{title:'The Lead',name:'Orion',color:'#e7a25d',index:2,prop:0,description:'Connecting the crew, sharing ideas, and keeping the work moving.',work:'Check in with the team'},
 developer:{title:'The Developer',name:'Pixel',color:'#a5a1de',index:3,prop:0,description:'Headphones on. Building, checking, and refining the next little feature.',work:'Work on a feature'},
 designer:{title:'The Designer',name:'Luna',color:'#e699ce',index:4,prop:6,description:'Sketching possibilities and finding a little magic in the details.',work:'Explore a design'},
 deployer:{title:'The Deployer',name:'Bolt',color:'#ead15e',index:5,prop:2,description:'Checking the systems and getting the crew’s next idea ready to go.',work:'Check the release'}
});
export const MAX_AGENTS=36,MAX_DESKS=48,COLUMNS=3;
export function project(x,y,z=0){return {x:(x-y)*40,y:(x+y)*20-z};}
export function stationPosition(index){return {x:2.2+(index%COLUMNS)*3.7,y:3.2+Math.floor(index/COLUMNS)*4};}
export function stationHome(index){const p=stationPosition(index),s=project(p.x,p.y);return {x:s.x+48,y:s.y+34};}
export function roomSize(count){return {width:12.4,length:Math.max(10.8,Math.ceil(count/COLUMNS)*4+3.2)};}
export class OfficeModel {
 constructor(saved=null){this.desks=[];this.agents=[];this.nextId=1;if(!this.restore(saved)){for(const role of Object.keys(ROLES))this.addAgent(role);}}
 addDesk(){if(this.desks.length>=MAX_DESKS)return null;const desk={id:'desk-'+this.nextId++,index:this.desks.length};this.desks.push(desk);return desk;}
 addAgent(role){if(!Object.hasOwn(ROLES,role)||this.agents.length>=MAX_AGENTS)return null;let desk=this.desks.find(d=>!this.agents.some(a=>a.deskId===d.id));if(!desk)desk=this.addDesk();if(!desk)return null;
  const base=ROLES[role].name,names=new Set(this.agents.map(a=>a.name));let name=base,i=2;while(names.has(name))name=base+' '+i++;
  const agent={id:'agent-'+this.nextId++,role,name,deskId:desk.id};this.agents.push(agent);return agent;
 }
 removeAgent(id){const index=this.agents.findIndex(a=>a.id===id);if(index<0)return false;this.agents.splice(index,1);return true;}
 deskFor(agent){return this.desks.find(d=>d.id===agent.deskId);}
 get room(){return roomSize(this.desks.length);}
 get emptyDesks(){return this.desks.length-this.agents.length;}
 serialize(){return {version:1,nextId:this.nextId,desks:this.desks,agents:this.agents};}
 restore(s){
  if(!s||s.version!==1||!Array.isArray(s.desks)||!Array.isArray(s.agents)||s.desks.length<1||s.desks.length>MAX_DESKS||s.agents.length>MAX_AGENTS||!Number.isSafeInteger(s.nextId)||s.nextId<1)return false;
  const ids=new Set(),occupied=new Set();let valid=true;
  const desks=s.desks.map((d,index)=>{if(typeof d?.id!=='string'||!/^desk-\d+$/.test(d.id)||ids.has(d.id)){valid=false;return null;}ids.add(d.id);return {id:d.id,index};});
  const deskIds=new Set(desks.filter(Boolean).map(d=>d.id));
  const agents=s.agents.map(a=>{if(typeof a?.id!=='string'||!/^agent-\d+$/.test(a.id)||ids.has(a.id)||!Object.hasOwn(ROLES,a.role)||!deskIds.has(a.deskId)||occupied.has(a.deskId)||typeof a.name!=='string'||a.name.length>40){valid=false;return null;}ids.add(a.id);occupied.add(a.deskId);return {id:a.id,role:a.role,name:a.name,deskId:a.deskId};});
  if(!valid)return false;
  this.desks=desks;this.agents=agents;this.nextId=Math.max(s.nextId,...[...ids].map(id=>Number(id.split('-')[1])+1));return true;
 }
}
export class Camera {
 constructor(){this.width=1000;this.height=650;this.scale=1;this.x=500;this.y=140;this.fitScale=1;}
 fit(room,width=this.width,height=this.height){this.width=width;this.height=height;const minX=-room.length*40-65,maxX=room.width*40+65,minY=-170,maxY=(room.width+room.length)*20+65;
  this.fitScale=Math.min((width-40)/(maxX-minX),(height-35)/(maxY-minY));this.scale=Math.min(1.4,this.fitScale);this.x=width/2-(minX+maxX)/2*this.scale;this.y=height/2-(minY+maxY)/2*this.scale;
 }
 screen(p){return {x:p.x*this.scale+this.x,y:p.y*this.scale+this.y};}
 world(p){return {x:(p.x-this.x)/this.scale,y:(p.y-this.y)/this.scale};}
 zoom(factor,at={x:this.width/2,y:this.height/2}){const w=this.world(at);this.scale=Math.max(this.fitScale*.55,Math.min(3,this.scale*factor));this.x=at.x-w.x*this.scale;this.y=at.y-w.y*this.scale;}
 pan(dx,dy){this.x+=dx;this.y+=dy;}
}
