import {ROLES,project,stationPosition} from './office-model.js';
export const ACTOR_SCALE=.72;
const WIDTHS=[176,132,83,152,83,180,166,70,57];
const poly=(c,points,fill)=>{c.beginPath();points.forEach((p,i)=>i?c.lineTo(p.x,p.y):c.moveTo(p.x,p.y));c.closePath();c.fillStyle=fill;c.fill();};
export class RoomRenderer {
 constructor({floor,furniture,rigs}){this.floor=floor;this.furniture=furniture.parts;this.rigs=rigs;}
 prop(c,index,x,y,width=WIDTHS[index],alpha=1){const image=this.furniture[index],height=image.height/image.width*width;c.save();c.globalAlpha=alpha;c.imageSmoothingQuality='high';c.drawImage(image,x-width/2,y-height,width,height);c.restore();}
 shadow(c,x,y,width){c.save();c.translate(x,y);c.scale(width*.42,width*.15);const g=c.createRadialGradient(0,0,0,0,0,1);g.addColorStop(0,'#211b245c');g.addColorStop(.4,'#211b2434');g.addColorStop(1,'#211b2400');c.fillStyle=g;c.beginPath();c.arc(0,0,1,0,Math.PI*2);c.fill();c.restore();}
 draw(c,model,brains,camera,{time=0,selected=null,hovered=null,width=1000,height=650,dpr=1}={}){
  c.setTransform(dpr,0,0,dpr,0,0);c.clearRect(0,0,width,height);
  const backdrop=c.createRadialGradient(width*.5,height*.45,10,width*.5,height*.5,Math.max(width,height)*.7);backdrop.addColorStop(0,'#53473f');backdrop.addColorStop(.55,'#322f32');backdrop.addColorStop(1,'#20212c');c.fillStyle=backdrop;c.fillRect(0,0,width,height);
  c.translate(camera.x,camera.y);c.scale(camera.scale,camera.scale);
  const room=model.room,w=room.width,l=room.length,a=project(0,0),b=project(w,0),d=project(0,l),e=project(w,l),down=p=>({x:p.x,y:p.y+17}),up=p=>({x:p.x,y:p.y-130});
  c.save();c.shadowColor='#0006';c.shadowBlur=45;poly(c,[a,b,e,d],'#34313b');c.restore();
  poly(c,[d,e,down(e),down(d)],'#393741');poly(c,[b,e,down(e),down(b)],'#302e38');
  const left=c.createLinearGradient(d.x,0,0,0);left.addColorStop(0,'#cbbb9f');left.addColorStop(1,'#e8dcc7');poly(c,[a,d,up(d),up(a)],left);
  const right=c.createLinearGradient(0,-100,b.x,b.y);right.addColorStop(0,'#efe2cb');right.addColorStop(1,'#d7c5a9');poly(c,[a,b,up(b),up(a)],right);
  c.strokeStyle='#f6ead250';c.lineWidth=3;c.beginPath();c.moveTo(up(d).x,up(d).y);c.lineTo(up(a).x,up(a).y);c.lineTo(up(b).x,up(b).y);c.stroke();
  c.save();c.transform(40,20,-40,20,0,0);c.beginPath();c.rect(0,0,w,l);c.clip();
  for(let x=0;x<w;x+=4)for(let y=0;y<l;y+=4)c.drawImage(this.floor,x,y,4,4);
  // Long soft window light over the textured tiles.
  for(let y=1.5;y<l-1;y+=3.6){c.fillStyle='#ffe1a31b';c.beginPath();c.moveTo(0,y);c.lineTo(5,y+1.6);c.lineTo(5,y+2.8);c.lineTo(0,y+1.2);c.closePath();c.fill();
   c.strokeStyle='#3325170b';c.lineWidth=.05;for(let k=0;k<8;k++){c.beginPath();c.moveTo(0,y+k*.15);c.lineTo(5,y+1.6+k*.15);c.stroke();}}
  c.restore();
  // Wooden skirting grounds the room and can extend with every new row.
  c.strokeStyle='#967c5c';c.lineWidth=6;c.beginPath();c.moveTo(d.x,d.y-2);c.lineTo(a.x,a.y-2);c.lineTo(b.x,b.y-2);c.stroke();
  for(let y=1.7;y<l-.8;y+=3.6){const p=project(0,y);this.prop(c,7,p.x+5,p.y-9);}
  const sign=project(5.5,0,72);c.save();c.translate(sign.x,sign.y);c.transform(1,.5,0,1,0,0);c.fillStyle='#806f58';c.font='600 12px Arial';c.textAlign='center';c.fillText('A LITTLE WORLD OF BIG IDEAS',0,0);c.restore();
  this.wires(c,model,time);
  const items=[];
  const addProp=(index,x,y,width=WIDTHS[index],depth=y)=>{items.push({depth,draw:()=>this.prop(c,index,x,y,width),shadow:{x,y:y-3,width}});};
  let p=project(9.8,.7);addProp(3,p.x,p.y,150);p=project(11.4,1);addProp(4,p.x,p.y,78);
  p=project(.7,.6);addProp(4,p.x,p.y,78);
  p=project(.6,l-.6);addProp(4,p.x,p.y,89);p=project(w-.7,l-.7);addProp(4,p.x,p.y,82);
  for(const desk of model.desks){
   const world=stationPosition(desk.index),p=project(world.x,world.y),agent=model.agents.find(a=>a.deskId===desk.id),role=agent?ROLES[agent.role]:null,prop=role?.prop??0;
   if(prop===0||prop===5||prop===6)addProp(8,p.x-22,p.y-13,54,p.y-13);
   addProp(prop,p.x,p.y+25,WIDTHS[prop],p.y+16);
   if(!agent)items.push({depth:p.y+27,draw:()=>{c.save();c.translate(p.x+28,p.y+40);c.fillStyle='#9ee4e0';c.font='8px Arial';c.textAlign='center';c.fillText('AVAILABLE',0,0);c.restore();}});
  }
  for(const agent of model.agents){const brain=brains.get(agent.id);if(!brain)continue;items.push({depth:brain.y,draw:()=>{c.save();c.translate(brain.x,brain.y);c.scale(ACTOR_SCALE,ACTOR_SCALE);this.rigs[agent.role].draw(c,{...brain,x:0,y:0},{selected:agent.id===selected,hovered:agent.id===hovered});c.restore();}});}
  items.filter(i=>i.shadow).forEach(i=>this.shadow(c,i.shadow.x,i.shadow.y,i.shadow.width));
  items.sort((a,b)=>a.depth-b.depth).forEach(i=>i.draw());
  // Warm floating dust remains subtle; the characters supply the main movement.
  for(let i=0;i<16;i++){const q=project(.8+(i%4)*2,1+((i*1.7+time*.07)%(l-1)));c.globalAlpha=.12*(.5+.5*Math.sin(time+i));c.fillStyle='#fff0cc';c.beginPath();c.arc(q.x,q.y-35-Math.sin(time*.5+i)*6,1,0,Math.PI*2);c.fill();}c.globalAlpha=1;
 }
 wires(c,model,time){
  const rows=Math.ceil(model.desks.length/3),paths=[];
  paths.push([project(1.0,1),project(1.0,model.room.length-1)]);
  for(let row=0;row<rows;row++){const y=4.7+row*4;paths.push([project(1,y),project(10.9,y)]);}
  for(const [i,path] of paths.entries()){
   [-2,0,2].forEach((offset,k)=>{c.save();c.strokeStyle=['#ecc78d99','#a7eef0cc','#96d7f4aa'][k];c.lineWidth=1.2;c.shadowColor='#9bdeff';c.shadowBlur=4;c.beginPath();c.moveTo(path[0].x+offset,path[0].y);c.lineTo(path[1].x+offset,path[1].y);c.stroke();c.restore();});
   const f=(time*.10+i*.29)%1,x=path[0].x+(path[1].x-path[0].x)*f,y=path[0].y+(path[1].y-path[0].y)*f;
   c.save();c.fillStyle='#ceffff';c.shadowColor='#8ae8ff';c.shadowBlur=9;c.beginPath();c.ellipse(x,y,3.3,1.8,.46,0,Math.PI*2);c.fill();c.restore();
  }
 }
}
