export class AmbientClock {
  constructor() { this.elapsed = 0; this.last = null; }
  tick(now) {
    if (this.last !== null) this.elapsed += Math.max(0, Math.min((now - this.last) / 1000, .15));
    this.last = now;
    return this.elapsed;
  }
  suspend() { this.last = null; }
}

export function cubic(a,b,c,d,t) {
  const u=1-t;
  return {x:u*u*u*a[0]+3*u*u*t*b[0]+3*u*t*t*c[0]+t*t*t*d[0],y:u*u*u*a[1]+3*u*u*t*b[1]+3*u*t*t*c[1]+t*t*t*d[1]};
}
