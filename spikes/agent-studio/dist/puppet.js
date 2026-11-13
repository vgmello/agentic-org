const DEFAULT_FACTORY=(w,h)=>{const c=document.createElement('canvas');c.width=w;c.height=h;return c;};
const ANCHORS={
  2:[[.34,.22],[.83,.86]],3:[[.49,.16],[.49,.85]],
  4:[[.59,.22],[.32,.86]],5:[[.4,.16],[.59,.85]],
  6:[[.58,.19],[.60,.85]],7:[[.47,.18],[.51,.88]],
  8:[[.49,.19],[.62,.85]],9:[[.49,.18],[.51,.88]]
};

export class SpriteAtlas {
  constructor(image,makeCanvas=DEFAULT_FACTORY,columns=4,rows=3,cleanIslands=false) {
    this.parts=[];
    const cw=image.width/columns,ch=image.height/rows;
    for(let index=0;index<columns*rows;index++) {
      const canvas=makeCanvas(Math.ceil(cw),Math.ceil(ch)),ctx=canvas.getContext('2d');
      ctx.drawImage(image,index%columns*cw,Math.floor(index/columns)*ch,cw,ch,0,0,canvas.width,canvas.height);
      const pixels=ctx.getImageData(0,0,canvas.width,canvas.height);
      let minX=canvas.width,minY=canvas.height,maxX=0,maxY=0;
      for(let y=0;y<canvas.height;y++)for(let x=0;x<canvas.width;x++) {
        const at=(y*canvas.width+x)*4;
        // The generated atlas has near-transparent color outside the silhouettes.
        // Discard that noise in the runtime texture; retain antialiased edges.
        if(pixels.data[at+3]<12){pixels.data[at+3]=0;continue;}
        if(pixels.data[at+3]>70){minX=Math.min(minX,x);minY=Math.min(minY,y);maxX=Math.max(maxX,x);maxY=Math.max(maxY,y);}
      }
      if(cleanIslands){
        removeSmallIslands(pixels.data,canvas.width,canvas.height);
        minX=canvas.width;minY=canvas.height;maxX=0;maxY=0;
        for(let y=0;y<canvas.height;y++)for(let x=0;x<canvas.width;x++)if(pixels.data[(y*canvas.width+x)*4+3]>70){minX=Math.min(minX,x);minY=Math.min(minY,y);maxX=Math.max(maxX,x);maxY=Math.max(maxY,y);}
      }
      ctx.putImageData(pixels,0,0);
      if(minX>=maxX||minY>=maxY)throw new Error(`Missing sprite part ${index}`);
      minX=Math.max(0,minX-3);minY=Math.max(0,minY-3);maxX=Math.min(canvas.width-1,maxX+3);maxY=Math.min(canvas.height-1,maxY+3);
      const cropped=makeCanvas(maxX-minX+1,maxY-minY+1);
      cropped.getContext('2d').drawImage(canvas,minX,minY,cropped.width,cropped.height,0,0,cropped.width,cropped.height);
      this.parts.push(cropped);
    }
  }
}

export class PuppetRenderer {
  constructor(atlas,head=null,torso=null){this.parts=atlas.parts;this.head=head||atlas.parts[0];this.torso=torso||atlas.parts[1];}
  bone(ctx,index,length,angle) {
    const part=this.parts[index],[s,e]=ANCHORS[index];
    const sx=s[0]*part.width,sy=s[1]*part.height,ex=e[0]*part.width,ey=e[1]*part.height;
    const sourceAngle=Math.atan2(ey-sy,ex-sx),scale=length/Math.hypot(ex-sx,ey-sy);
    ctx.save();ctx.rotate(angle);ctx.scale(index>=6?1.3:1.12,1);ctx.rotate(Math.PI/2-sourceAngle);ctx.scale(scale,scale);ctx.drawImage(part,-sx,-sy);ctx.restore();
    ctx.translate(-Math.sin(angle)*length,Math.cos(angle)*length);ctx.rotate(angle);
  }
  leg(ctx,side,pose) {
    const left=side==='left',part=left?6:8;
    ctx.save();ctx.translate(left?-11:12,-67);
    this.bone(ctx,part,26,pose[left?'leftLeg':'rightLeg']);
    this.bone(ctx,left?7:9,27,pose[left?'leftKnee':'rightKnee']);
    const boot=this.parts[left?10:11],w=28,h=boot.height/boot.width*w;
    ctx.rotate(-pose[left?'leftLeg':'rightLeg']-pose[left?'leftKnee':'rightKnee']);
    ctx.drawImage(boot,-w*(left?.57:.45),-h*.17,w,h);ctx.restore();
  }
  arm(ctx,side,pose) {
    const left=side==='left';ctx.save();ctx.translate(left?-22:23,-117);
    this.bone(ctx,left?2:4,27,pose[left?'leftArm':'rightArm']);
    this.bone(ctx,left?3:5,30,pose[left?'leftElbow':'rightElbow']);ctx.restore();
  }
  draw(ctx,brain,{selected=false,hovered=false}={}) {
    const p=brain.pose,t=brain.time,x=brain.x,y=brain.y;
    // Contact shadow follows the feet, independently of the animated body.
    ctx.save();ctx.translate(x,y);ctx.fillStyle='#19132145';ctx.beginPath();ctx.ellipse(0,3,30,9,0,0,Math.PI*2);ctx.fill();
    if(selected||hovered){ctx.strokeStyle=selected?'#96effac9':'#a8f6ff70';ctx.lineWidth=1.2;ctx.shadowColor='#69daff';ctx.shadowBlur=7;ctx.beginPath();ctx.ellipse(0,2,34,12,0,0,Math.PI*2);ctx.stroke();ctx.shadowBlur=0;}
    ctx.restore();
    ctx.save();ctx.translate(x,y+p.bob);ctx.scale(1.07,1.07);ctx.rotate(p.body);ctx.imageSmoothingQuality='high';
    this.leg(ctx,'right',p);this.leg(ctx,'left',p);
    this.arm(ctx,'right',p);
    const torso=this.torso;ctx.drawImage(torso,-27,-133,54,69);
    this.arm(ctx,'left',p);
    ctx.save();ctx.translate(p.headX,-124);ctx.rotate(p.head);
    const head=this.head,w=77,h=head.height/head.width*w;
    ctx.drawImage(head,-w*.51,-h+5,w,h);
    // Blink uses the same head transform so the eyes follow every gesture.
    const blinkPhase=(t+2.9)%5.6,blink=Math.max(0,1-Math.abs(blinkPhase-.15)/.105);
    if(blink>0){ctx.save();ctx.fillStyle=`rgba(19,41,63,${blink*.97})`;ctx.beginPath();ctx.ellipse(4,-h*.37,6.8,6.7,0,0,Math.PI*2);ctx.ellipse(23,-h*.41,5.1,6.2,0,0,Math.PI*2);ctx.fill();ctx.restore();}
    ctx.restore();ctx.restore();
    if(p.hologram>.02)this.hologram(ctx,brain,p.hologram);
  }
  hologram(ctx,brain,alpha) {
    const x=brain.x+44,y=brain.y-116,t=brain.time;
    ctx.save();ctx.globalAlpha=alpha*.7;ctx.translate(x,y);ctx.transform(1,.26,0,1,0,0);
    const flicker=.8+Math.sin(t*2)*.1;ctx.fillStyle=`rgba(79,198,234,${.10*flicker})`;ctx.strokeStyle='#9deffd';ctx.lineWidth=.7;ctx.shadowColor='#65dfff';ctx.shadowBlur=5;
    ctx.beginPath();ctx.roundRect(-26,-31,52,62,3);ctx.fill();ctx.stroke();
    for(let i=0;i<3;i++){ctx.strokeRect(-18+i*13,-20+i*10,12,13);ctx.beginPath();ctx.moveTo(-12+i*13,-7+i*10);ctx.lineTo(-12+i*13,5+i*10);ctx.stroke();}
    const scan=(t*.25%1)*54-27;ctx.globalAlpha*=.6;ctx.beginPath();ctx.moveTo(-23,scan);ctx.lineTo(23,scan);ctx.stroke();ctx.restore();
  }
}

// Discard small disconnected fragments at atlas cell boundaries.
function removeSmallIslands(data,width,height){
 const labels=new Int32Array(width*height),queue=new Int32Array(width*height),areas=[0];let id=0,largest=0;
 for(let start=0;start<labels.length;start++){
  if(labels[start]||data[start*4+3]<12)continue;
  id++;let read=0,write=1;queue[0]=start;labels[start]=id;
  while(read<write){const at=queue[read++],x=at%width,y=Math.floor(at/width);
   for(let dy=-1;dy<=1;dy++)for(let dx=-1;dx<=1;dx++){const nx=x+dx,ny=y+dy;if(nx<0||ny<0||nx>=width||ny>=height)continue;const n=ny*width+nx;if(!labels[n]&&data[n*4+3]>=12){labels[n]=id;queue[write++]=n;}}
  }
  areas[id]=write;largest=Math.max(largest,write);
 }
 const minimum=Math.max(32,largest*.004);
 for(let i=0;i<labels.length;i++)if(areas[labels[i]]<minimum)data[i*4+3]=0;
}
