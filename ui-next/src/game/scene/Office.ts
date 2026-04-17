import { Container, Graphics, Text, TextStyle, Ticker } from 'pixi.js';

export type DecorKind = 'command' | 'computers' | 'desks' | 'rack' | 'gears' | 'monitors';

export interface DepartmentInput {
  id: string;
  label: string;
  decor: DecorKind;
  agents: string[];
  color: number;
}

interface Cloud {
  g: Graphics;
  baseX: number;
  y: number;
  drift: number;
}

interface Room {
  id: string;
  label: string;
  decor: DecorKind;
  agents: string[];
  color: number;
  container: Container;
  decorContainer: Container;
  x: number;
  width: number;
  floorY: number;
  ceilingY: number;
  animState: {
    sweep?: Graphics;
    gearA?: Container;
    gearB?: Container;
    leds?: { g: Graphics; phase: number }[];
    graphs?: { g: Graphics; x: number; y: number; w: number; h: number; phase: number }[];
    cursors?: { g: Graphics; offset: number }[];
  };
}

// Cenário: prédio em corte lateral. Céu + skyline no topo, salas lado a lado,
// chão/calçada embaixo. Cada sala é um departamento com decor próprio e
// posições alocadas para os sprites dos agentes.
export class Office extends Container {
  private sky: Graphics;
  private skyline: Graphics;
  private cloudLayer: Container;
  private clouds: Cloud[] = [];
  private roof: Graphics;
  private building: Container;
  private groundG: Graphics;
  private rooms = new Map<string, Room>();
  private positions = new Map<string, [number, number]>();
  private pendingDepts: DepartmentInput[] | null = null;
  private w = 0;
  private h = 0;
  private t = 0;

  private readonly SKY_H = 64;
  private readonly FLOOR_OFFSET = 42;

  constructor() {
    super();
    this.sky = new Graphics();
    this.skyline = new Graphics();
    this.cloudLayer = new Container();
    this.roof = new Graphics();
    this.building = new Container();
    this.groundG = new Graphics();
    this.addChild(this.sky);
    this.addChild(this.cloudLayer);
    this.addChild(this.skyline);
    this.addChild(this.roof);
    this.addChild(this.building);
    this.addChild(this.groundG);
  }

  resize(w: number, h: number) {
    this.w = w;
    this.h = h;
    this.drawSky();
    this.drawSkyline();
    this.drawRoof();
    this.seedClouds();
    this.layoutRooms();
    this.drawGround();
    if (this.pendingDepts) {
      const d = this.pendingDepts;
      this.pendingDepts = null;
      this.setDepartments(d);
    }
  }

  setDepartments(depts: DepartmentInput[]) {
    if (this.w === 0 || this.h === 0) {
      this.pendingDepts = depts;
      return;
    }
    this.building.removeChildren();
    this.rooms.clear();
    for (const d of depts) {
      const container = new Container();
      const decorContainer = new Container();
      this.building.addChild(container);
      this.rooms.set(d.id, {
        id: d.id,
        label: d.label,
        decor: d.decor,
        agents: [...d.agents],
        color: d.color,
        container,
        decorContainer,
        x: 0,
        width: 0,
        floorY: 0,
        ceilingY: 0,
        animState: {},
      });
    }
    this.layoutRooms();
  }

  positionFor(name: string): [number, number] | null {
    return this.positions.get(name) || null;
  }

  // limites físicos da sala à qual o agente pertence (usados pela física do canvas)
  getRoomBounds(name: string): { x1: number; x2: number; ceilingY: number; floorY: number } | null {
    for (const r of this.rooms.values()) {
      if (r.agents.includes(name)) {
        return {
          x1: r.x + 14,
          x2: r.x + r.width - 14,
          ceilingY: r.ceilingY,
          floorY: r.floorY,
        };
      }
    }
    return null;
  }

  tick(t: Ticker) {
    this.t += t.deltaTime;
    for (const c of this.clouds) {
      c.baseX -= c.drift * t.deltaTime;
      if (c.baseX < -60) c.baseX = this.w + 60;
      c.g.x = c.baseX;
    }
    for (const r of this.rooms.values()) {
      const s = r.animState;
      if (r.decor === 'command' && s.sweep) s.sweep.rotation = this.t * 0.03;
      if (r.decor === 'gears') {
        if (s.gearA) s.gearA.rotation = this.t * 0.02;
        if (s.gearB) s.gearB.rotation = -this.t * 0.028;
      }
      if (r.decor === 'rack' && s.leds) {
        for (const led of s.leds) {
          const v = 0.35 + Math.max(0, Math.sin(this.t * 0.08 + led.phase)) * 0.65;
          led.g.alpha = v;
        }
      }
      if (r.decor === 'monitors' && s.graphs) {
        for (const gph of s.graphs) {
          gph.g.clear();
          gph.g.moveTo(gph.x, gph.y);
          for (let xx = 0; xx < gph.w; xx += 2) {
            const yy = gph.y + Math.sin(xx * 0.35 + this.t * 0.08 + gph.phase) * (gph.h / 3);
            gph.g.lineTo(gph.x + xx, yy);
          }
          gph.g.stroke({ width: 1, color: 0x22c55e, alpha: 0.85 });
        }
      }
      if (r.decor === 'computers' && s.cursors) {
        for (const c of s.cursors) {
          const on = Math.floor((this.t + c.offset) / 30) % 2 === 0;
          c.g.alpha = on ? 1 : 0.15;
        }
      }
    }
  }

  // ── cenário fixo ───────────────────────────────────────────────
  private drawSky() {
    this.sky.clear();
    const bands = 16;
    for (let i = 0; i < bands; i++) {
      const t = i / (bands - 1);
      const c = lerpColor(0xbfdbfe, 0xfde4ff, t);
      this.sky.rect(0, (this.SKY_H * i) / bands, this.w, this.SKY_H / bands + 1).fill({ color: c });
    }
  }

  private drawSkyline() {
    this.skyline.clear();
    const baseY = this.SKY_H - 2;
    let x = 0;
    let seed = 1;
    const rnd = () => {
      seed = (seed * 9301 + 49297) % 233280;
      return seed / 233280;
    };
    while (x < this.w) {
      const bw = 20 + rnd() * 30;
      const bh = 14 + rnd() * 22;
      this.skyline.rect(x, baseY - bh, bw, bh).fill({ color: 0x64748b, alpha: 0.55 });
      for (let yy = 4; yy < bh - 2; yy += 4) {
        for (let xx = 2; xx < bw - 3; xx += 3) {
          if (rnd() > 0.45)
            this.skyline.rect(x + xx, baseY - bh + yy, 1, 1).fill({ color: 0xfef3c7, alpha: 0.85 });
        }
      }
      x += bw + 2;
    }
  }

  private drawRoof() {
    this.roof.clear();
    this.roof.rect(0, this.SKY_H, this.w, 6).fill({ color: 0x1e293b });
    this.roof.rect(0, this.SKY_H + 6, this.w, 2).fill({ color: 0x334155 });
  }

  private seedClouds() {
    this.cloudLayer.removeChildren();
    this.clouds = [];
    const n = Math.max(2, Math.floor(this.w / 260));
    for (let i = 0; i < n; i++) {
      const g = new Graphics();
      const r = 8 + Math.random() * 5;
      const y = 6 + Math.random() * (this.SKY_H - 20);
      const baseX = Math.random() * this.w;
      g.circle(0, 0, r).fill({ color: 0xffffff, alpha: 0.85 });
      g.circle(r * 0.8, -r * 0.1, r * 0.7).fill({ color: 0xffffff, alpha: 0.85 });
      g.circle(-r * 0.8, -r * 0.05, r * 0.65).fill({ color: 0xffffff, alpha: 0.85 });
      g.x = baseX;
      g.y = y;
      this.cloudLayer.addChild(g);
      this.clouds.push({ g, baseX, y, drift: 0.04 + Math.random() * 0.06 });
    }
  }

  private drawGround() {
    const floorY = this.h - this.FLOOR_OFFSET;
    this.groundG.clear();
    this.groundG.rect(0, floorY + 6, this.w, this.FLOOR_OFFSET).fill({ color: 0x475569 });
    this.groundG.rect(0, floorY + 6, this.w, 2).fill({ color: 0x334155 });
    for (let x = 0; x < this.w; x += 22) {
      this.groundG.rect(x, floorY + 10, 1, this.FLOOR_OFFSET - 6).fill({ color: 0x334155, alpha: 0.4 });
    }
  }

  // ── salas ──────────────────────────────────────────────────────
  private layoutRooms() {
    this.positions.clear();
    if (this.rooms.size === 0 || this.w === 0 || this.h === 0) return;

    const ceilY = this.SKY_H + 8;
    const floorY = this.h - this.FLOOR_OFFSET;

    const list = [...this.rooms.values()];
    const totalWeight = list.reduce((s, r) => s + Math.max(1.3, r.agents.length), 0);
    const available = this.w;

    let x = 0;
    const minRoomW = Math.min(150, Math.floor(this.w / list.length));
    for (const r of list) {
      const weight = Math.max(1.3, r.agents.length) / totalWeight;
      const width = Math.max(minRoomW, Math.floor(available * weight));
      r.x = x;
      r.width = width;
      r.ceilingY = ceilY;
      r.floorY = floorY;
      this.drawRoom(r, r.id === list[list.length - 1].id, r.id === list[0].id);
      x += width;
    }

    // ajusta última sala pra preencher resto exato da tela
    const last = list[list.length - 1];
    if (last && last.x + last.width < this.w) {
      last.width = this.w - last.x;
      this.drawRoom(last, true, list.length === 1);
    }
  }

  private drawRoom(r: Room, isLast: boolean, isFirst: boolean) {
    r.container.removeChildren();
    r.decorContainer.removeChildren();
    r.animState = {};

    const wallColor = lerpColor(r.color, 0xffffff, 0.82);
    const wallShade = lerpColor(r.color, 0xffffff, 0.7);
    const floorColor = lerpColor(r.color, 0x334155, 0.65);
    const ceilColor = lerpColor(r.color, 0x0f172a, 0.5);

    const bg = new Graphics();
    bg.rect(r.x, r.ceilingY, r.width, r.floorY - r.ceilingY).fill({ color: wallColor });
    // rodapé mais escuro
    bg.rect(r.x, r.floorY - 14, r.width, 14).fill({ color: wallShade });
    // linha de rodapé
    bg.rect(r.x, r.floorY - 14, r.width, 1).fill({ color: 0x334155, alpha: 0.45 });
    // teto
    bg.rect(r.x, r.ceilingY, r.width, 4).fill({ color: ceilColor });
    // chão
    bg.rect(r.x, r.floorY, r.width, 6).fill({ color: floorColor });
    // paredes laterais
    if (isFirst) {
      bg.rect(r.x, r.ceilingY, 2, r.floorY - r.ceilingY + 6).fill({ color: 0x334155, alpha: 0.7 });
    }
    if (!isLast) {
      bg.rect(r.x + r.width - 2, r.ceilingY, 2, r.floorY - r.ceilingY + 6)
        .fill({ color: 0x334155, alpha: 0.7 });
    } else {
      bg.rect(r.x + r.width - 2, r.ceilingY, 2, r.floorY - r.ceilingY + 6)
        .fill({ color: 0x334155, alpha: 0.7 });
    }
    r.container.addChild(bg);

    // placa com nome do departamento
    const label = new Text({
      text: r.label,
      style: new TextStyle({
        fontSize: 11,
        fill: 0xf8fafc,
        fontWeight: '700',
        fontFamily: 'ui-sans-serif, system-ui',
      }),
    });
    label.anchor.set(0.5, 0);
    label.position.set(r.x + r.width / 2, r.ceilingY + 6);
    const signBg = new Graphics();
    const sw = label.width + 16;
    signBg
      .roundRect(r.x + r.width / 2 - sw / 2, r.ceilingY + 4, sw, 16, 5)
      .fill({ color: r.color })
      .stroke({ width: 1, color: 0x0f172a, alpha: 0.4 });
    r.container.addChild(signBg);
    r.container.addChild(label);

    // decor da sala
    this.drawDecor(r);
    r.container.addChild(r.decorContainer);

    this.allocateAgents(r);
  }

  private drawDecor(r: Room) {
    const d = r.decorContainer;
    const inner = {
      x1: r.x + 8,
      x2: r.x + r.width - 8,
      y1: r.ceilingY + 26,
      y2: r.floorY - 8,
    };

    switch (r.decor) {
      case 'computers': {
        const n = Math.max(1, r.agents.length);
        const slots = computeSlots(n, r.width, r.x, r.floorY);
        r.animState.cursors = [];
        for (const [sx] of slots) {
          const g = new Graphics();
          // monitor atrás do agente
          g.rect(sx - 14, r.floorY - 58, 28, 30).fill({ color: 0x0f172a });
          g.rect(sx - 12, r.floorY - 56, 24, 26).fill({ color: 0x0ea5e9, alpha: 0.55 });
          for (let i = 0; i < 4; i++) {
            const lw = 4 + Math.random() * 16;
            g.rect(sx - 10, r.floorY - 54 + i * 4, lw, 1.4).fill({ color: 0x67e8f9 });
          }
          // base/pé do monitor
          g.rect(sx - 3, r.floorY - 30, 6, 6).fill({ color: 0x334155 });
          g.rect(sx - 10, r.floorY - 25, 20, 2).fill({ color: 0x334155 });
          // mesa
          g.rect(sx - 22, r.floorY - 22, 44, 4).fill({ color: 0x57534e });
          g.rect(sx - 20, r.floorY - 18, 3, 14).fill({ color: 0x44403c });
          g.rect(sx + 17, r.floorY - 18, 3, 14).fill({ color: 0x44403c });
          // teclado
          g.rect(sx - 14, r.floorY - 24, 28, 2).fill({ color: 0x0f172a });
          d.addChild(g);
          // cursor piscando
          const cursor = new Graphics();
          cursor.rect(0, 0, 2, 4).fill({ color: 0xfef08a });
          cursor.x = sx + 2;
          cursor.y = r.floorY - 50;
          d.addChild(cursor);
          r.animState.cursors.push({ g: cursor, offset: Math.random() * 60 });
        }
        break;
      }
      case 'command': {
        const cx = r.x + r.width / 2;
        const cy = inner.y1 + 28;
        // radar
        const radar = new Graphics();
        radar.circle(cx, cy, 26).stroke({ width: 1.5, color: 0x475569, alpha: 0.75 });
        radar.circle(cx, cy, 18).stroke({ width: 1, color: 0x475569, alpha: 0.6 });
        radar.circle(cx, cy, 10).stroke({ width: 1, color: 0x475569, alpha: 0.55 });
        radar.moveTo(cx - 26, cy).lineTo(cx + 26, cy).stroke({ width: 0.8, color: 0x475569, alpha: 0.4 });
        radar.moveTo(cx, cy - 26).lineTo(cx, cy + 26).stroke({ width: 0.8, color: 0x475569, alpha: 0.4 });
        radar.circle(cx, cy, 2).fill({ color: 0x22c55e });
        d.addChild(radar);
        // ponteiro
        const sweep = new Graphics();
        sweep
          .moveTo(0, 0)
          .lineTo(0, -26)
          .lineTo(7, -24)
          .closePath()
          .fill({ color: 0x22c55e, alpha: 0.55 });
        sweep.x = cx;
        sweep.y = cy;
        d.addChild(sweep);
        r.animState.sweep = sweep;
        // pins vermelhos
        const dots = new Graphics();
        for (let i = 0; i < 5; i++) {
          const a = Math.random() * Math.PI * 2;
          const rr = 5 + Math.random() * 20;
          dots.circle(cx + Math.cos(a) * rr, cy + Math.sin(a) * rr, 1.2)
            .fill({ color: 0xef4444, alpha: 0.8 });
        }
        d.addChild(dots);
        break;
      }
      case 'desks': {
        const cx = r.x + r.width / 2;
        const g = new Graphics();
        // quadro na parede
        g.rect(r.x + 12, inner.y1 + 2, 34, 24).fill({ color: 0xf3f4f6 })
          .stroke({ width: 1, color: 0x57534e });
        g.moveTo(r.x + 16, inner.y1 + 20)
          .lineTo(r.x + 24, inner.y1 + 10)
          .lineTo(r.x + 32, inner.y1 + 18)
          .lineTo(r.x + 40, inner.y1 + 12)
          .stroke({ width: 1.2, color: 0x2563eb });
        // mesa grande
        g.rect(cx - 34, r.floorY - 22, 68, 4).fill({ color: 0x78716c });
        g.rect(cx - 32, r.floorY - 18, 3, 14).fill({ color: 0x57534e });
        g.rect(cx + 29, r.floorY - 18, 3, 14).fill({ color: 0x57534e });
        // luminária
        g.rect(cx - 26, r.floorY - 44, 2, 22).fill({ color: 0x0f172a });
        g.moveTo(cx - 28, r.floorY - 44)
          .lineTo(cx - 16, r.floorY - 44)
          .lineTo(cx - 18, r.floorY - 36)
          .lineTo(cx - 26, r.floorY - 36)
          .closePath()
          .fill({ color: 0xfef3c7 });
        // livros
        g.rect(cx + 10, r.floorY - 36, 4, 14).fill({ color: 0xef4444 });
        g.rect(cx + 15, r.floorY - 36, 4, 14).fill({ color: 0x2563eb });
        g.rect(cx + 20, r.floorY - 34, 4, 12).fill({ color: 0x16a34a });
        // xícara
        g.rect(cx - 6, r.floorY - 30, 6, 8).fill({ color: 0xffffff })
          .stroke({ width: 1, color: 0x475569 });
        d.addChild(g);
        break;
      }
      case 'rack': {
        const cx = r.x + r.width / 2;
        const rackW = Math.min(70, r.width - 24);
        const rackH = 72;
        const rackX = cx - rackW / 2;
        const rackY = r.floorY - rackH - 14;
        const g = new Graphics();
        g.roundRect(rackX, rackY, rackW, rackH, 3)
          .fill({ color: 0x1e293b })
          .stroke({ width: 1, color: 0x0f172a });
        for (let row = 0; row < 7; row++) {
          const yy = rackY + 5 + row * 9;
          g.rect(rackX + 4, yy, rackW - 8, 6).fill({ color: 0x334155 });
          // ventilação
          for (let vx = 0; vx < rackW - 14; vx += 3) {
            g.rect(rackX + 6 + vx, yy + 2, 1, 2).fill({ color: 0x0f172a });
          }
        }
        d.addChild(g);
        // LEDs piscando (animados)
        r.animState.leds = [];
        for (let row = 0; row < 7; row++) {
          const led = new Graphics();
          led.circle(0, 0, 1.8).fill({ color: row === 2 ? 0xef4444 : 0x22c55e });
          led.x = rackX + rackW - 6;
          led.y = rackY + 8 + row * 9;
          d.addChild(led);
          r.animState.leds.push({ g: led, phase: Math.random() * Math.PI * 2 });
        }
        // cabos
        const cables = new Graphics();
        cables.moveTo(rackX, rackY + rackH - 4)
          .quadraticCurveTo(rackX - 10, r.floorY - 10, rackX - 4, r.floorY - 2)
          .stroke({ width: 2, color: 0x0f172a });
        cables.moveTo(rackX + rackW, rackY + rackH - 8)
          .quadraticCurveTo(rackX + rackW + 12, r.floorY - 12, rackX + rackW + 6, r.floorY - 2)
          .stroke({ width: 2, color: 0x0f172a });
        d.addChild(cables);
        break;
      }
      case 'gears': {
        const cx = r.x + r.width / 2;
        const cy = inner.y1 + 32;
        const gearA = makeGear(0x94a3b8, 22, 10);
        gearA.x = cx - 18;
        gearA.y = cy;
        const gearB = makeGear(0xfbbf24, 15, 8);
        gearB.x = cx + 14;
        gearB.y = cy + 6;
        d.addChild(gearA);
        d.addChild(gearB);
        r.animState.gearA = gearA;
        r.animState.gearB = gearB;
        // painel com manômetro
        const panel = new Graphics();
        panel.roundRect(r.x + r.width - 42, r.floorY - 30, 34, 20, 3)
          .fill({ color: 0x0f172a });
        panel.circle(r.x + r.width - 30, r.floorY - 20, 6)
          .stroke({ width: 1, color: 0xfbbf24 });
        panel.moveTo(r.x + r.width - 30, r.floorY - 20)
          .lineTo(r.x + r.width - 26, r.floorY - 24)
          .stroke({ width: 1.2, color: 0xef4444 });
        d.addChild(panel);
        break;
      }
      case 'monitors': {
        const cols = Math.max(2, Math.min(4, Math.floor((r.width - 20) / 40)));
        const rows = 2;
        const cellW = Math.floor((r.width - 20) / cols);
        const cellH = 22;
        const startX = r.x + 10;
        const startY = inner.y1 + 4;
        r.animState.graphs = [];
        for (let i = 0; i < rows; i++) {
          for (let j = 0; j < cols; j++) {
            const xx = startX + j * cellW;
            const yy = startY + i * (cellH + 4);
            const bg = new Graphics();
            bg.roundRect(xx, yy, cellW - 4, cellH, 2)
              .fill({ color: 0x0f172a })
              .stroke({ width: 1, color: 0x334155 });
            // grid
            bg.moveTo(xx + 2, yy + cellH / 2).lineTo(xx + cellW - 6, yy + cellH / 2)
              .stroke({ width: 0.4, color: 0x475569, alpha: 0.6 });
            d.addChild(bg);
            const graph = new Graphics();
            d.addChild(graph);
            r.animState.graphs = r.animState.graphs || [];
            r.animState.graphs.push({
              g: graph,
              x: xx + 3,
              y: yy + cellH / 2,
              w: cellW - 10,
              h: cellH - 6,
              phase: Math.random() * 10,
            });
          }
        }
        break;
      }
    }
  }

  private allocateAgents(r: Room) {
    const n = r.agents.length;
    if (n === 0) return;
    const slots = computeSlots(n, r.width, r.x, r.floorY);
    r.agents.forEach((name, i) => {
      this.positions.set(name, slots[i] || [r.x + r.width / 2, r.floorY - 30]);
    });
  }
}

// distribui N slots dentro da sala — em linha, com wrap para outra fileira se
// não couber.
function computeSlots(n: number, width: number, startX: number, floorY: number): [number, number][] {
  const pad = 18;
  const slotW = 56;
  const maxCols = Math.max(1, Math.floor((width - pad * 2) / slotW));
  const cols = Math.min(n, maxCols);
  const rows = Math.ceil(n / cols);
  const cellW = (width - pad * 2) / cols;
  const out: [number, number][] = [];
  for (let i = 0; i < n; i++) {
    const col = i % cols;
    const row = Math.floor(i / cols);
    const x = startX + pad + cellW * (col + 0.5);
    // fileira mais próxima do chão primeiro; fileiras adicionais empilham pra cima
    const y = floorY - 30 - (rows - 1 - row) * 56;
    out.push([x, y]);
  }
  return out;
}

function makeGear(color: number, R: number, teeth: number): Container {
  const c = new Container();
  const g = new Graphics();
  for (let i = 0; i < teeth; i++) {
    const a = (i / teeth) * Math.PI * 2;
    const x = Math.cos(a) * R;
    const y = Math.sin(a) * R;
    g.rect(x - 1.6, y - 1.6, 3.2, 3.2).fill({ color });
  }
  g.circle(0, 0, R * 0.78).fill({ color }).stroke({ width: 1, color: 0x0f172a });
  g.circle(0, 0, R * 0.3).fill({ color: 0x0f172a });
  c.addChild(g);
  return c;
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
