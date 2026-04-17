import { Container, Graphics, Ticker } from 'pixi.js';

interface Cloud {
  g: Graphics;
  baseX: number;
  y: number;
  drift: number;
}

// Parallax inspirado no Spine Boy: 3 camadas com velocidades distintas
// expostas por positionX getter/setter — permite que o AgentCanvas
// mova a câmera horizontalmente e cada camada reaja na sua cadência.
export class Backdrop extends Container {
  private sky: Graphics;
  private far: Graphics;
  private near: Graphics;
  private ground: Graphics;
  private grid: Graphics;
  private cloudLayer: Container;
  private clouds: Cloud[] = [];
  private w = 0;
  private h = 0;
  private x0 = 0; // deslocamento virtual de câmera

  private readonly SPEED_FAR = 0.1;
  private readonly SPEED_NEAR = 0.25;
  private readonly SPEED_GROUND = 1.0;

  constructor() {
    super();
    this.sky = new Graphics();
    this.far = new Graphics();
    this.near = new Graphics();
    this.ground = new Graphics();
    this.grid = new Graphics();
    this.cloudLayer = new Container();
    this.addChild(this.sky);
    this.addChild(this.far);
    this.addChild(this.cloudLayer);
    this.addChild(this.near);
    this.addChild(this.ground);
    this.addChild(this.grid);
  }

  resize(w: number, h: number) {
    this.w = w;
    this.h = h;
    this.drawSky();
    this.drawHills();
    this.drawGroundLine();
    this.drawGrid();
    this.seedClouds();
  }

  private drawSky() {
    this.sky.clear();
    const bands = 24;
    for (let i = 0; i < bands; i++) {
      const t = i / (bands - 1);
      const c = lerpColor(0xeaf4ff, 0xfdf4ff, t);
      this.sky.rect(0, (this.h * i) / bands, this.w, this.h / bands + 1).fill({ color: c });
    }
  }

  private drawHills() {
    this.far.clear();
    this.near.clear();
    const baseY = this.h - 60;
    // camada distante (duas cópias para loop infinito no scroll)
    for (let k = 0; k < 2; k++) {
      const offset = k * this.w;
      this.far.moveTo(offset - 20, baseY);
      for (let x = 0; x <= this.w + 40; x += 30) {
        const y = baseY - 28 - Math.sin((x + k * 133) * 0.02) * 14 - Math.cos(x * 0.009) * 6;
        this.far.lineTo(offset + x, y);
      }
      this.far.lineTo(offset + this.w + 20, this.h);
      this.far.lineTo(offset - 20, this.h);
      this.far.closePath();
      this.far.fill({ color: 0xd6e4f5, alpha: 0.75 });
    }
    // camada próxima
    for (let k = 0; k < 2; k++) {
      const offset = k * this.w;
      this.near.moveTo(offset - 20, baseY + 10);
      for (let x = 0; x <= this.w + 40; x += 24) {
        const y = baseY + 4 - Math.abs(Math.sin((x + k * 71) * 0.03)) * 22;
        this.near.lineTo(offset + x, y);
      }
      this.near.lineTo(offset + this.w + 20, this.h);
      this.near.lineTo(offset - 20, this.h);
      this.near.closePath();
      this.near.fill({ color: 0xc7dafc, alpha: 0.85 });
    }
  }

  private drawGroundLine() {
    this.ground.clear();
    const y = this.h - 28;
    this.ground.rect(-this.w, y, this.w * 3, this.h).fill({ color: 0xe8eefb });
    this.ground.moveTo(-this.w, y).lineTo(this.w * 2, y).stroke({ width: 1.2, color: 0x94a3b8, alpha: 0.35 });
  }

  private drawGrid() {
    this.grid.clear();
    const step = 28;
    const c = 0xcbd5e1;
    const startX = -((this.x0 * this.SPEED_GROUND) % step);
    for (let x = startX; x < this.w + step; x += step) {
      this.grid.moveTo(x, 0).lineTo(x, this.h).stroke({ width: 1, color: c, alpha: 0.28 });
    }
    for (let y = step; y < this.h; y += step) {
      this.grid.moveTo(0, y).lineTo(this.w, y).stroke({ width: 1, color: c, alpha: 0.28 });
    }
  }

  private seedClouds() {
    this.cloudLayer.removeChildren();
    this.clouds = [];
    const n = Math.max(3, Math.floor(this.w / 220));
    for (let i = 0; i < n; i++) {
      const g = new Graphics();
      const r = 14 + Math.random() * 10;
      const y = 20 + Math.random() * (this.h * 0.3);
      const baseX = Math.random() * this.w;
      g.circle(0, 0, r).fill({ color: 0xffffff, alpha: 0.9 });
      g.circle(r * 0.8, -r * 0.2, r * 0.8).fill({ color: 0xffffff, alpha: 0.9 });
      g.circle(-r * 0.8, -r * 0.1, r * 0.7).fill({ color: 0xffffff, alpha: 0.9 });
      g.x = baseX;
      g.y = y;
      this.cloudLayer.addChild(g);
      this.clouds.push({ g, baseX, y, drift: 0.05 + Math.random() * 0.08 });
    }
  }

  // getter/setter inspirado no tutorial: a câmera virtual move cada camada na sua cadência
  get positionX() {
    return this.x0;
  }
  set positionX(v: number) {
    this.x0 = v;
    this.far.x = -v * this.SPEED_FAR;
    this.near.x = -v * this.SPEED_NEAR;
    this.ground.x = -v * this.SPEED_GROUND;
    this.drawGrid();
    for (const c of this.clouds) {
      c.g.x = c.baseX - v * this.SPEED_FAR * 0.6;
    }
  }

  tick(t: Ticker) {
    // deriva lenta das nuvens mesmo sem câmera
    for (const c of this.clouds) {
      c.baseX -= c.drift * t.deltaTime;
      if (c.baseX < -60) c.baseX = this.w + 60;
      c.g.x = c.baseX - this.x0 * this.SPEED_FAR * 0.6;
    }
  }
}

function lerpColor(a: number, b: number, t: number): number {
  const ar = (a >> 16) & 0xff;
  const ag = (a >> 8) & 0xff;
  const ab = a & 0xff;
  const br = (b >> 16) & 0xff;
  const bg = (b >> 8) & 0xff;
  const bb = b & 0xff;
  const r = Math.round(ar + (br - ar) * t);
  const g = Math.round(ag + (bg - ag) * t);
  const bl = Math.round(ab + (bb - ab) * t);
  return (r << 16) | (g << 8) | bl;
}
