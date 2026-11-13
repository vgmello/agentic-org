const clamp=(v,a,b)=>Math.max(a,Math.min(b,v));
const ease=t=>{t=clamp(t,0,1);return t*t*(3-2*t)};
const lerp=(a,b,t)=>a+(b-a)*t;
const ROUTINE=[
  {kind:'work',duration:5.8},
  {kind:'walk',duration:2.8,to:[278,603]},
  {kind:'think',duration:3.2},
  {kind:'inspect',duration:4.6},
  {kind:'walk',duration:3.2,to:[234,584]},
  {kind:'work',duration:5.4},
  {kind:'stretch',duration:3.2},
  {kind:'walk',duration:2.2,to:[249,594]}
];
const LABELS={hello:'Saying hello',work:'Refining the blueprint',walk:'Taking a few steps',think:'Thinking it through',inspect:'Checking the design',stretch:'Taking a little breather'};
const POSE={head:0,headX:0,body:0,bob:0,leftArm:.12,leftElbow:-.14,rightArm:-.14,rightElbow:.1,leftLeg:0,leftKnee:0,rightLeg:0,rightKnee:0,hologram:0};

// One agent, one deterministic state machine. The same rig can drive more actors.
export class AgentBrain {
  constructor({x=249,y=594,seed=0,role='architect',name='Nova'}={}) {
    this.time=seed*1.7;this.x=x;this.y=y;this.home={x,y};this.role=role;this.name=name;this.routineIndex=seed%ROUTINE.length;this.completed=0;
    this.pose={...POSE};this.gaze=0;this.hover=false;this.history=[];
    this.state={kind:'hello',duration:3.4,elapsed:0,from:[this.x,this.y]};
    this.bubble='Hey, I’m '+name+'!';this.bubbleUntil=this.time+3.2;
    if(seed>0){const spec=ROUTINE[this.routineIndex++%ROUTINE.length];this.begin({...spec,to:spec.to?[this.home.x+spec.to[0]-249,this.home.y+spec.to[1]-594]:undefined});this.state.elapsed=seed%2;this.bubbleUntil=0;}
  }
  begin(spec,interactive=false) {
    this.state={...spec,elapsed:0,from:[this.x,this.y]};
    if(interactive){this.history.unshift({at:this.time,text:this.status});this.history.length=Math.min(4,this.history.length);}
  }
  request(action) {
    if(action==='hello'){this.begin({kind:'hello',duration:3.6},true);this.bubble='Hello! Good to see you.';}
    else if(action==='blueprint'){this.begin({kind:'work',duration:6.5},true);this.bubble={director:'Here’s the next milestone.',architect:'Let me show you the plan.',lead:'Let’s connect the dots.',developer:'One feature coming right up.',designer:'I have an idea to sketch.',deployer:'Let’s run the release checks.'}[this.role];}
    else if(action==='break'){this.begin({kind:'walk',duration:3,to:[this.x>this.home.x+7?this.home.x-15:this.home.x+29,this.x>this.home.x+7?this.home.y-10:this.home.y+9],then:'stretch'},true);this.bubble='A little stretch, then back to it.';}
    else return false;
    this.bubbleUntil=this.time+3.5;return true;
  }
  get status(){if(this.state.kind==='work')return {director:'Planning the next milestone',architect:'Refining the blueprint',lead:'Checking in with the crew',developer:'Building a feature',designer:'Sketching a new idea',deployer:'Checking the release'}[this.role];return LABELS[this.state.kind]}
  get progress(){return clamp(this.state.elapsed/this.state.duration,0,1)}
  setGaze(worldX){this.gaze=clamp((worldX-this.x)/130,-1,1)}
  update(dt) {
    dt=clamp(dt,0,.15);this.time+=dt;this.state.elapsed+=dt;
    if(this.state.elapsed>=this.state.duration) {
      if(this.state.kind==='work'||this.state.kind==='inspect'){
        this.completed++;this.history.unshift({at:this.time,text:this.state.kind==='work'?{director:'Next milestone planned',architect:'Blueprint updated',lead:'Team check-in complete',developer:'Feature iteration complete',designer:'A new sketch is ready',deployer:'Release checks complete'}[this.role]:'Workspace check complete'});this.history.length=Math.min(4,this.history.length);
      }
      const next=this.state.then?{kind:this.state.then,duration:3.5}:ROUTINE[this.routineIndex++%ROUTINE.length];this.begin({...next,to:next.to?[this.home.x+next.to[0]-249,this.home.y+next.to[1]-594]:undefined});
    }
    const s=this.state,t=s.elapsed,phase=t*4.8;
    const target={...POSE,head:Math.sin(this.time*.85)*.065+this.gaze*.055,headX:this.gaze*2.5,bob:Math.sin(this.time*2.1)*.75,body:Math.sin(this.time*.8)*.025};
    if(s.kind==='walk') {
      const f=ease(t/s.duration);this.x=lerp(s.from[0],s.to[0],f);this.y=lerp(s.from[1],s.to[1],f);
      const speed=Math.sin(Math.PI*clamp(t/s.duration,0,1));
      target.leftLeg=Math.sin(phase)*.23*speed;target.rightLeg=-target.leftLeg;
      target.leftKnee=Math.max(0,-Math.sin(phase))*.3*speed;target.rightKnee=Math.max(0,Math.sin(phase))*.3*speed;
      target.leftArm=.12-Math.sin(phase)*.22*speed;target.rightArm=-.12+Math.sin(phase)*.22*speed;
      target.bob=-Math.abs(Math.sin(phase))*2.8*speed;target.body=(s.to[0]>s.from[0]?-.045:.045)*speed;
      target.head=Math.sin(phase*.5)*.035;
    } else if(s.kind==='hello') {
      target.rightArm=-2.05;target.rightElbow=-.65+Math.sin(t*9)*.42;
      target.head=Math.sin(t*4)*.08;target.body=-.035;target.bob=-Math.sin(Math.min(t,1)*Math.PI)*2.5;
    } else if(s.kind==='work') {
      target.leftArm=1.25+Math.sin(t*1.7)*.12;target.leftElbow=.32+Math.sin(t*2.2)*.1;
      target.head=-.14+Math.sin(t*2)*.045;target.body=.035;target.rightElbow=.33;
      target.hologram=ease(t/1.2)*.6;
      if(this.role==='developer'||this.role==='designer'){target.rightArm=1.0;target.rightElbow=.55+Math.sin(t*9)*.10;target.leftElbow=.3+Math.sin(t*11)*.10;target.head=-.06;target.hologram=.15;}
      if(this.role==='lead'){target.leftArm=.32;target.leftElbow=-1.3;target.rightArm=-1.0;target.rightElbow=-.3+Math.sin(t*2)*.2;}
      if(this.role==='deployer'){target.leftArm=.25;target.leftElbow=-1.1;target.rightArm=.4;target.rightElbow=-1.3;target.hologram=.3;}
    } else if(s.kind==='think') {
      target.rightArm=-.28;target.rightElbow=-2.45;target.head=.16;target.headX=-2;
      target.body=.035;target.leftElbow=.12;
    } else if(s.kind==='inspect') {
      target.rightArm=-.7;target.rightElbow=-1.18;target.leftArm=.42;target.leftElbow=.65;
      target.head=.12+Math.sin(t*1.5)*.05;target.hologram=1;
    } else if(s.kind==='stretch') {
      target.leftArm=1.8;target.rightArm=-1.8;target.leftElbow=.45;target.rightElbow=-.45;
      target.head=Math.sin(t*1.8)*.13;target.body=Math.sin(t*1.8)*.07;target.bob=-2;
    }
    const blend=1-Math.exp(-dt*8.5);
    for(const key of Object.keys(POSE))this.pose[key]=lerp(this.pose[key],target[key],blend);
    return this.pose;
  }
}
export function agentHitBox(brain){return {x:brain.x-83,y:brain.y-215,width:166,height:220};}
export function pointInAgent(brain,x,y){const b=agentHitBox(brain);return x>=b.x&&x<=b.x+b.width&&y>=b.y&&y<=b.y+b.height;}
