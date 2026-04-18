import { Container, Graphics, Text, TextStyle, Ticker } from 'pixi.js';

export type DecorKind = 'command' | 'computers' | 'desks' | 'rack' | 'gears' | 'monitors';

export interface DepartmentInput {
  id: string;
  label: string;
  decor: DecorKind;
  agents: string[];
  color: number;
  floor: number;
}

interface Cloud {
  g: Graphics;
  baseX: number;
  y: number;
  drift: number;
}

interface Lamp {
  // Cada lâmpada tem a cúpula (fixa) e o halo (alpha varia com a hora).
  fixture: Graphics;
  bulb: Graphics;
  halo: Graphics;
}

interface WindowRect {
  cx: number;
  cy: number;
  w: number;
  h: number;
}

interface Room {
  id: string;
  label: string;
  decor: DecorKind;
  agents: string[];
  color: number;
  floor: number;
  container: Container;
  decorContainer: Container;
  lampsContainer: Container;
  lamps: Lamp[];
  windows: WindowRect[];
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

type DayPhase = 'night' | 'dawn' | 'morning' | 'midday' | 'afternoon' | 'dusk';

interface DayState {
  hours: number;       // hora fracionária 0..24
  phase: DayPhase;
  isDay: boolean;      // sol visível (~6h-18h)
  sunAlt: number;      // 0..1: altura do sol no céu
  moonAlt: number;     // 0..1: altura da lua
  sunX: number;        // 0..1: posição horizontal (L→O)
  moonX: number;       // 0..1
  lampIntensity: number; // 0 (dia pleno) → 1 (noite)
}

function computeDaylight(now = new Date()): DayState {
  const h = now.getHours() + now.getMinutes() / 60 + now.getSeconds() / 3600;
  const isDay = h >= 6 && h < 18;
  const sunAlt = isDay ? Math.sin(((h - 6) / 12) * Math.PI) : 0;
  let moonAlt = 0;
  if (!isDay) {
    const nh = (h + 24 - 18) % 24; // 0..12 (18h→0, 06h→12)
    moonAlt = Math.sin((nh / 12) * Math.PI);
  }
  const sunX = (h - 6) / 12;
  const moonX = ((h + 24 - 18) % 24) / 12;
  let phase: DayPhase;
  if (h >= 5 && h < 7) phase = 'dawn';
  else if (h >= 7 && h < 11) phase = 'morning';
  else if (h >= 11 && h < 15) phase = 'midday';
  else if (h >= 15 && h < 17) phase = 'afternoon';
  else if (h >= 17 && h < 19) phase = 'dusk';
  else phase = 'night';
  // Lâmpadas ligam gradualmente no crepúsculo e ficam plenas à noite.
  let lampIntensity = 0;
  if (phase === 'night') lampIntensity = 1;
  else if (phase === 'dawn') lampIntensity = Math.max(0, 1 - (h - 5) / 2);
  else if (phase === 'dusk') lampIntensity = Math.max(0, (h - 17) / 2);
  else if (phase === 'afternoon') lampIntensity = Math.max(0, (h - 16.5) / 2);
  return { hours: h, phase, isDay, sunAlt, moonAlt, sunX, moonX, lampIntensity };
}

// Gradiente do céu (topo → horizonte) por fase.
const SKY_GRADIENT: Record<DayPhase, [number, number]> = {
  night:     [0x0a0f1f, 0x1e1b4b],
  dawn:      [0x1e1b4b, 0xfbbf77],
  morning:   [0xbfdbfe, 0xfde4ff],
  midday:    [0x93c5fd, 0xdbeafe],
  afternoon: [0xfdba74, 0xbae6fd],
  dusk:      [0x7c3aed, 0xf97316],
  // night repetido: usado quando fora das faixas
};

// Cenário 2D multi-andar (plataforma). Céu + skyline no topo, andares empilhados,
// calçada/chão embaixo. O mundo pode ser maior que o viewport — câmera/scroll
// fica a cargo do chamador (agentsPixi). Cada andar distribui salas horizontalmente.
export class Office extends Container {
  private sky: Graphics;
  private starsG: Graphics;
  private sunG: Graphics;
  private moonG: Graphics;
  private skyline: Graphics;
  private cloudLayer: Container;
  private clouds: Cloud[] = [];
  private roof: Graphics;
  private building: Container;
  private groundG: Graphics;
  private floorDividers: Graphics;
  private nightTint: Graphics;
  private celestialInWindow: Graphics;
  private rooms = new Map<string, Room>();
  private positions = new Map<string, [number, number]>();
  private pendingDepts: DepartmentInput[] | null = null;
  private viewportW = 0;
  private viewportH = 0;
  private worldW = 0;
  private worldH = 0;
  private numFloors = 0;
  private t = 0;
  private day: DayState = computeDaylight();
  private dayTickAccum = 999; // força update no primeiro tick

  private readonly SKY_H = 64;
  private readonly ROOF_H = 8;
  private readonly FLOOR_H = 196;
  private readonly FLOOR_OFFSET = 42;
  private readonly MIN_ROOM_W = 220;

  constructor() {
    super();
    this.sky = new Graphics();
    this.starsG = new Graphics();
    this.sunG = new Graphics();
    this.moonG = new Graphics();
    this.skyline = new Graphics();
    this.cloudLayer = new Container();
    this.roof = new Graphics();
    this.building = new Container();
    this.floorDividers = new Graphics();
    this.groundG = new Graphics();
    this.nightTint = new Graphics();
    this.celestialInWindow = new Graphics();
    this.addChild(this.sky);
    this.addChild(this.starsG);
    this.addChild(this.sunG);
    this.addChild(this.moonG);
    this.addChild(this.cloudLayer);
    this.addChild(this.skyline);
    this.addChild(this.roof);
    this.addChild(this.building);
    this.addChild(this.celestialInWindow);
    this.addChild(this.floorDividers);
    this.addChild(this.groundG);
    this.addChild(this.nightTint);
  }

  resize(w: number, h: number) {
    this.viewportW = w;
    this.viewportH = h;
    if (this.pendingDepts) {
      const d = this.pendingDepts;
      this.pendingDepts = null;
      this.setDepartments(d);
      return;
    }
    this.relayoutAll();
  }

  getWorldSize(): { width: number; height: number } {
    return {
      width: this.worldW || this.viewportW,
      height: this.worldH || this.viewportH,
    };
  }

  setDepartments(depts: DepartmentInput[]) {
    if (this.viewportW === 0 || this.viewportH === 0) {
      this.pendingDepts = depts;
      return;
    }
    this.building.removeChildren();
    this.rooms.clear();
    for (const d of depts) {
      const container = new Container();
      const decorContainer = new Container();
      const lampsContainer = new Container();
      this.building.addChild(container);
      this.rooms.set(d.id, {
        id: d.id,
        label: d.label,
        decor: d.decor,
        agents: [...d.agents],
        color: d.color,
        floor: d.floor,
        container,
        decorContainer,
        lampsContainer,
        lamps: [],
        windows: [],
        x: 0,
        width: 0,
        floorY: 0,
        ceilingY: 0,
        animState: {},
      });
    }
    this.relayoutAll();
  }

  positionFor(name: string): [number, number] | null {
    return this.positions.get(name) || null;
  }

  // Posição de um humano dentro da sala do departamento indicado.
  // Humanos ficam próximos à porta (canto inferior direito), empilhando à esquerda
  // conforme o `index` — 0 é o mais próximo da porta.
  humanPositionFor(roomId: string, index: number): [number, number] | null {
    const r = this.rooms.get(roomId);
    if (!r) return null;
    const doorW = 28;
    const doorMarginRight = 14;
    const doorX = r.x + r.width - doorMarginRight - doorW;
    const spacing = 34;
    const offsetX = 22 + index * spacing;
    const x = Math.max(r.x + 26, doorX - offsetX);
    const y = r.floorY - 30;
    return [x, y];
  }

  hasRoom(roomId: string): boolean {
    return this.rooms.has(roomId);
  }

  // Acha o Y do centro do sprite humano quando ele pousa num piso embaixo
  // da coordenada (x, y) dada. Se nenhum quarto tem piso abaixo, usa o chão do mundo.
  // O offset FEET_OFFSET compensa a diferença entre o ponto âncora do sprite
  // (centro do corpo) e a base dos pés (~30px abaixo).
  landingYAt(x: number, y: number, feetOffset = 30): number {
    const worldFloorY = this.worldH - this.FLOOR_OFFSET;
    let candidate = worldFloorY;
    for (const r of this.rooms.values()) {
      if (x < r.x || x > r.x + r.width) continue;
      if (r.floorY < y + feetOffset) continue; // piso está acima dos pés (já estamos abaixo dele)
      if (r.floorY < candidate) candidate = r.floorY;
    }
    return candidate - feetOffset;
  }

  // limites físicos da sala à qual o agente pertence (world coords)
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
    // Atualiza ciclo dia/noite a cada ~2s para economizar desenho.
    this.dayTickAccum += t.deltaTime;
    if (this.dayTickAccum >= 120) {
      this.dayTickAccum = 0;
      this.refreshDaylight();
    }
    for (const c of this.clouds) {
      c.baseX -= c.drift * t.deltaTime;
      if (c.baseX < -60) c.baseX = this.worldW + 60;
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

  // ── layout geral ─────────────────────────────────────────────
  private relayoutAll() {
    this.computeWorldSize();
    this.day = computeDaylight();
    this.drawSky();
    this.drawStars();
    this.drawSun();
    this.drawMoon();
    this.drawSkyline();
    this.drawRoof();
    this.seedClouds();
    this.layoutRooms();
    this.drawFloorDividers();
    this.drawGround();
    this.drawNightTint();
    this.drawCelestialInWindows();
  }

  private computeWorldSize() {
    const byFloor = new Map<number, Room[]>();
    for (const r of this.rooms.values()) {
      if (!byFloor.has(r.floor)) byFloor.set(r.floor, []);
      byFloor.get(r.floor)!.push(r);
    }
    this.numFloors = byFloor.size;
    let maxFloorMin = 0;
    for (const rooms of byFloor.values()) {
      maxFloorMin = Math.max(maxFloorMin, rooms.length * this.MIN_ROOM_W);
    }
    this.worldW = Math.max(this.viewportW, maxFloorMin || this.viewportW);
    if (this.numFloors === 0) {
      this.worldH = this.viewportH;
    } else {
      this.worldH = Math.max(
        this.viewportH,
        this.SKY_H + this.ROOF_H + this.numFloors * this.FLOOR_H + this.FLOOR_OFFSET,
      );
    }
  }

  // ── cenário ──────────────────────────────────────────────────
  private drawSky() {
    this.sky.clear();
    const [top, bot] = SKY_GRADIENT[this.day.phase] || SKY_GRADIENT.midday;
    const bands = 16;
    for (let i = 0; i < bands; i++) {
      const t = i / (bands - 1);
      const c = lerpColor(top, bot, t);
      this.sky
        .rect(0, (this.SKY_H * i) / bands, this.worldW, this.SKY_H / bands + 1)
        .fill({ color: c });
    }
  }

  private drawStars() {
    this.starsG.clear();
    // Visível só à noite / aurora / crepúsculo com baixa intensidade.
    const alpha = this.day.phase === 'night' ? 1
      : (this.day.phase === 'dawn' || this.day.phase === 'dusk') ? 0.35
      : 0;
    if (alpha === 0) { this.starsG.alpha = 0; return; }
    this.starsG.alpha = alpha;
    let seed = 42;
    const rnd = () => {
      seed = (seed * 9301 + 49297) % 233280;
      return seed / 233280;
    };
    const count = Math.max(30, Math.floor(this.worldW / 18));
    for (let i = 0; i < count; i++) {
      const x = rnd() * this.worldW;
      const y = rnd() * (this.SKY_H - 6);
      const r = rnd() > 0.9 ? 1.4 : 0.7;
      const a = 0.5 + rnd() * 0.5;
      this.starsG.circle(x, y, r).fill({ color: 0xffffff, alpha: a });
    }
  }

  private drawSun() {
    this.sunG.clear();
    if (!this.day.isDay || this.day.sunAlt <= 0) { this.sunG.alpha = 0; return; }
    this.sunG.alpha = 1;
    // Arco semicircular: x vai da esquerda à direita; y mais alto quanto maior sunAlt.
    const margin = 40;
    const x = margin + (this.worldW - margin * 2) * this.day.sunX;
    const ySkyBot = this.SKY_H - 2;
    const y = ySkyBot - Math.max(6, this.day.sunAlt * (this.SKY_H - 14));
    const R = 9;
    // Halo leve
    this.sunG.circle(x, y, R + 6).fill({ color: 0xfef3c7, alpha: 0.35 });
    this.sunG.circle(x, y, R + 3).fill({ color: 0xfde68a, alpha: 0.55 });
    // Corpo do sol — cor varia com a fase
    const sunColor = this.day.phase === 'dawn' || this.day.phase === 'dusk'
      ? 0xf97316
      : this.day.phase === 'afternoon'
        ? 0xfb923c
        : 0xfde047;
    this.sunG.circle(x, y, R).fill({ color: sunColor });
  }

  private drawMoon() {
    this.moonG.clear();
    if (this.day.isDay || this.day.moonAlt <= 0) { this.moonG.alpha = 0; return; }
    this.moonG.alpha = 1;
    const margin = 40;
    const x = margin + (this.worldW - margin * 2) * this.day.moonX;
    const ySkyBot = this.SKY_H - 2;
    const y = ySkyBot - Math.max(6, this.day.moonAlt * (this.SKY_H - 14));
    const R = 8;
    this.moonG.circle(x, y, R + 5).fill({ color: 0xe0e7ff, alpha: 0.18 });
    this.moonG.circle(x, y, R).fill({ color: 0xf1f5f9 });
    // Cratera pra dar textura
    this.moonG.circle(x - 2, y - 1, 1.4).fill({ color: 0xcbd5e1, alpha: 0.8 });
    this.moonG.circle(x + 2.5, y + 2, 0.9).fill({ color: 0xcbd5e1, alpha: 0.8 });
  }

  private drawNightTint() {
    this.nightTint.clear();
    if (this.numFloors === 0) return;
    // Camada escura aplicada sobre o prédio à noite para que as lâmpadas
    // sejam a fonte de luz visível. Alpha cresce com a intensidade de noite.
    const alpha = this.day.lampIntensity * 0.42;
    if (alpha <= 0) return;
    const y = this.SKY_H + this.ROOF_H;
    const h = this.numFloors * this.FLOOR_H;
    this.nightTint
      .rect(0, y, this.worldW, h)
      .fill({ color: 0x020617, alpha });
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
    while (x < this.worldW) {
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
    this.roof.rect(0, this.SKY_H, this.worldW, 6).fill({ color: 0x1e293b });
    this.roof.rect(0, this.SKY_H + 6, this.worldW, 2).fill({ color: 0x334155 });
  }

  private seedClouds() {
    this.cloudLayer.removeChildren();
    this.clouds = [];
    const n = Math.max(2, Math.floor(this.worldW / 260));
    for (let i = 0; i < n; i++) {
      const g = new Graphics();
      const r = 8 + Math.random() * 5;
      const y = 6 + Math.random() * (this.SKY_H - 20);
      const baseX = Math.random() * this.worldW;
      g.circle(0, 0, r).fill({ color: 0xffffff, alpha: 0.85 });
      g.circle(r * 0.8, -r * 0.1, r * 0.7).fill({ color: 0xffffff, alpha: 0.85 });
      g.circle(-r * 0.8, -r * 0.05, r * 0.65).fill({ color: 0xffffff, alpha: 0.85 });
      g.x = baseX;
      g.y = y;
      this.cloudLayer.addChild(g);
      this.clouds.push({ g, baseX, y, drift: 0.04 + Math.random() * 0.06 });
    }
  }

  private drawFloorDividers() {
    this.floorDividers.clear();
    if (this.numFloors <= 1) return;
    for (let i = 1; i < this.numFloors; i++) {
      const y = this.SKY_H + this.ROOF_H + i * this.FLOOR_H - 6;
      this.floorDividers.rect(0, y, this.worldW, 6).fill({ color: 0x1f2937 });
      this.floorDividers.rect(0, y + 6, this.worldW, 2).fill({ color: 0x0f172a, alpha: 0.7 });
    }
  }

  private drawGround() {
    const floorY = this.worldH - this.FLOOR_OFFSET;
    this.groundG.clear();
    this.groundG.rect(0, floorY + 6, this.worldW, this.FLOOR_OFFSET).fill({ color: 0x475569 });
    this.groundG.rect(0, floorY + 6, this.worldW, 2).fill({ color: 0x334155 });
    for (let x = 0; x < this.worldW; x += 22) {
      this.groundG
        .rect(x, floorY + 10, 1, this.FLOOR_OFFSET - 6)
        .fill({ color: 0x334155, alpha: 0.4 });
    }
  }

  // ── salas ────────────────────────────────────────────────────
  private layoutRooms() {
    this.positions.clear();
    if (this.rooms.size === 0 || this.worldW === 0 || this.worldH === 0) return;

    const byFloor = new Map<number, Room[]>();
    for (const r of this.rooms.values()) {
      if (!byFloor.has(r.floor)) byFloor.set(r.floor, []);
      byFloor.get(r.floor)!.push(r);
    }

    const floorIdxs = [...byFloor.keys()].sort((a, b) => b - a); // decrescente: maior andar no topo
    const maxFloor = floorIdxs[0] ?? 0;

    for (const fIdx of floorIdxs) {
      const list = byFloor.get(fIdx)!;
      list.sort((a, b) => a.id.localeCompare(b.id));

      const slotIdxFromTop = maxFloor - fIdx; // 0 = topo (logo abaixo do telhado)
      const ceilY = this.SKY_H + this.ROOF_H + slotIdxFromTop * this.FLOOR_H;
      const floorY = ceilY + this.FLOOR_H - 6;

      const totalWeight = list.reduce((s, r) => s + Math.max(1.3, r.agents.length), 0);
      let x = 0;
      for (let i = 0; i < list.length; i++) {
        const r = list[i];
        const weight = Math.max(1.3, r.agents.length) / totalWeight;
        const width = Math.max(this.MIN_ROOM_W, Math.floor(this.worldW * weight));
        r.x = x;
        r.width = width;
        r.ceilingY = ceilY;
        r.floorY = floorY;
        const isFirst = i === 0;
        const isLast = i === list.length - 1;
        this.drawRoom(r, isLast, isFirst);
        x += width;
      }
      // ajusta última sala pra fechar até worldW
      const last = list[list.length - 1];
      if (last && last.x + last.width < this.worldW) {
        last.width = this.worldW - last.x;
        this.drawRoom(last, true, list.length === 1);
      }
    }
  }

  private drawRoom(r: Room, isLast: boolean, isFirst: boolean) {
    r.container.removeChildren();
    r.decorContainer.removeChildren();
    r.lampsContainer.removeChildren();
    r.lamps = [];
    r.animState = {};

    const wallColor = lerpColor(r.color, 0xffffff, 0.82);
    const wallShade = lerpColor(r.color, 0xffffff, 0.7);
    const floorColor = lerpColor(r.color, 0x334155, 0.65);
    const ceilColor = lerpColor(r.color, 0x0f172a, 0.5);

    const bg = new Graphics();
    bg.rect(r.x, r.ceilingY, r.width, r.floorY - r.ceilingY).fill({ color: wallColor });
    bg.rect(r.x, r.floorY - 14, r.width, 14).fill({ color: wallShade });
    bg.rect(r.x, r.floorY - 14, r.width, 1).fill({ color: 0x334155, alpha: 0.45 });
    bg.rect(r.x, r.ceilingY, r.width, 4).fill({ color: ceilColor });

    // Piso cerâmico: faixa base + grid de azulejos com destaque nas juntas.
    const tileH = 10;
    const tileW = 26;
    bg.rect(r.x, r.floorY, r.width, tileH).fill({ color: floorColor });
    bg.rect(r.x, r.floorY, r.width, 1).fill({ color: 0x0f172a, alpha: 0.45 });
    const tileDark = lerpColor(floorColor, 0x0f172a, 0.3);
    const tileLight = lerpColor(floorColor, 0xffffff, 0.15);
    for (let tx = 0; tx < r.width; tx += tileW) {
      // Alterna tom claro/escuro pra dar profundidade.
      const alt = ((tx / tileW) | 0) % 2 === 0;
      bg.rect(r.x + tx + 1, r.floorY + 2, tileW - 2, tileH - 3)
        .fill({ color: alt ? tileLight : tileDark, alpha: 0.18 });
      // Junta vertical
      bg.rect(r.x + tx, r.floorY + 1, 1, tileH - 1).fill({ color: 0x0f172a, alpha: 0.35 });
    }
    // Junta horizontal (topo do rodapé)
    bg.rect(r.x, r.floorY + tileH - 1, r.width, 1).fill({ color: 0x0f172a, alpha: 0.25 });

    if (isFirst) {
      bg.rect(r.x, r.ceilingY, 2, r.floorY - r.ceilingY + 6).fill({ color: 0x334155, alpha: 0.7 });
    }
    bg.rect(r.x + r.width - 2, r.ceilingY, 2, r.floorY - r.ceilingY + 6)
      .fill({ color: 0x334155, alpha: isLast ? 0.7 : 0.7 });
    r.container.addChild(bg);

    this.drawWindows(r);

    // Porta no canto inferior direito + placa logo acima.
    const doorW = 28;
    const doorH = 46;
    const doorMarginRight = 14;
    const doorX = r.x + r.width - doorMarginRight - doorW;
    const doorTop = r.floorY - doorH;

    const door = new Graphics();
    // Moldura / batente
    door
      .rect(doorX - 2, doorTop - 2, doorW + 4, doorH + 2)
      .fill({ color: 0x0f172a, alpha: 0.55 });
    // Folha da porta
    const doorFill = lerpColor(r.color, 0x1e293b, 0.55);
    door
      .rect(doorX, doorTop, doorW, doorH)
      .fill({ color: doorFill })
      .stroke({ width: 1, color: 0x0f172a, alpha: 0.7 });
    // Paineis decorativos
    door
      .rect(doorX + 4, doorTop + 5, doorW - 8, doorH * 0.38)
      .stroke({ width: 0.8, color: 0x0f172a, alpha: 0.35 });
    door
      .rect(doorX + 4, doorTop + doorH * 0.5, doorW - 8, doorH * 0.38)
      .stroke({ width: 0.8, color: 0x0f172a, alpha: 0.35 });
    // Maçaneta
    door
      .circle(doorX + doorW - 6, doorTop + doorH / 2 + 3, 1.6)
      .fill({ color: 0xfbbf24 });
    r.container.addChild(door);

    // Placa acima da porta
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
    const signH = 16;
    const signW = Math.max(doorW + 8, label.width + 14);
    const signX = doorX + doorW / 2 - signW / 2;
    const signY = doorTop - signH - 3;
    const signBg = new Graphics();
    signBg
      .roundRect(signX, signY, signW, signH, 4)
      .fill({ color: r.color })
      .stroke({ width: 1, color: 0x0f172a, alpha: 0.5 });
    // Cordinha presa no teto da placa
    signBg
      .rect(signX + 3, signY - 3, 1, 3)
      .fill({ color: 0x0f172a, alpha: 0.5 });
    signBg
      .rect(signX + signW - 4, signY - 3, 1, 3)
      .fill({ color: 0x0f172a, alpha: 0.5 });
    label.position.set(doorX + doorW / 2, signY + 2);
    r.container.addChild(signBg);
    r.container.addChild(label);

    this.drawDecor(r);
    r.container.addChild(r.decorContainer);

    this.drawLamps(r);
    r.container.addChild(r.lampsContainer);

    this.allocateAgents(r);
  }

  // ── janelas ───────────────────────────────────────────────────
  // Duas janelas na parede do fundo, acima do nível dos agentes, com
  // cor/brilho interno refletindo o céu atual (céu claro de dia, escuro
  // à noite quando as lâmpadas predominam).
  private drawWindows(r: Room) {
    r.windows = [];
    const doorW = 28;
    const doorMarginRight = 14;
    const doorX = r.x + r.width - doorMarginRight - doorW;

    // Área disponível pra janelas: descarta a faixa perto da porta.
    const avail = { left: r.x + 18, right: doorX - 18 };
    if (avail.right - avail.left < 80) return;

    const winH = 34;
    const winW = 46;
    const winY = r.ceilingY + 30;

    // Decide quantas janelas cabem (1 ou 2) centralizadas.
    const span = avail.right - avail.left;
    const count = span >= 2 * winW + 24 ? 2 : 1;
    const gap = count === 2 ? (span - 2 * winW) / 3 : 0;

    const windowG = new Graphics();
    for (let i = 0; i < count; i++) {
      const x = count === 2
        ? avail.left + gap + i * (winW + gap)
        : avail.left + (span - winW) / 2;
      // Moldura
      windowG
        .rect(x - 2, winY - 2, winW + 4, winH + 4)
        .fill({ color: 0x334155 });
      // Vidro — tom translúcido neutro (azul-acinzentado), independente do céu.
      // Não compete com o cenário, só sugere "janela".
      windowG.rect(x, winY, winW, winH).fill({ color: 0xbfdbfe, alpha: 0.35 });
      // Brilho horizontal sutil no topo do vidro
      windowG.rect(x + 2, winY + 2, winW - 4, 2).fill({ color: 0xffffff, alpha: 0.25 });
      // Cruz (caixilho)
      windowG.rect(x + winW / 2 - 1, winY, 2, winH).fill({ color: 0x334155 });
      windowG.rect(x, winY + winH / 2 - 1, winW, 2).fill({ color: 0x334155 });
      // Peitoril
      windowG
        .rect(x - 4, winY + winH + 2, winW + 8, 3)
        .fill({ color: 0x475569 });

      r.windows.push({ cx: x + winW / 2, cy: winY + winH / 2, w: winW, h: winH });
    }
    r.container.addChild(windowG);
  }

  // ── sol/lua discreto em no máximo uma janela por sala ─────────
  // A janela escolhida por sala é a mais próxima (horizontalmente) do astro real.
  // O astro é pequeno e translúcido pra não roubar atenção do escritório.
  private drawCelestialInWindows() {
    this.celestialInWindow.clear();
    const d = this.day;
    const showSun = d.isDay && d.sunAlt > 0;
    const showMoon = !d.isDay && d.moonAlt > 0;
    if (!showSun && !showMoon) return;

    const margin = 40;
    const celestialX = showSun
      ? margin + (this.worldW - margin * 2) * d.sunX
      : margin + (this.worldW - margin * 2) * d.moonX;

    for (const r of this.rooms.values()) {
      if (r.windows.length === 0) continue;
      // Melhor janela desta sala: menor distância horizontal ao astro.
      let best: WindowRect | null = null;
      let bestDist = Infinity;
      for (const w of r.windows) {
        const dist = Math.abs(w.cx - celestialX);
        if (dist < bestDist) { bestDist = dist; best = w; }
      }
      if (!best) continue;
      // Só desenha se o astro está razoavelmente acima/próximo desta sala.
      if (bestDist > best.w * 3.5) continue;
      this.renderCelestialDot(best, showSun, celestialX);
    }
  }

  private renderCelestialDot(win: WindowRect, isSun: boolean, celestialX: number) {
    const g = this.celestialInWindow;
    const halfW = win.w / 2 - 4;
    // Leve deslocamento horizontal seguindo a direção do astro (sutil).
    const offsetX = Math.max(-halfW, Math.min(halfW, (celestialX - win.cx) * 0.15));
    const cx = win.cx + offsetX;
    const cy = win.cy - win.h * 0.1; // ligeiramente acima do centro
    const R = Math.min(win.w, win.h) * 0.12; // bem menor

    if (isSun) {
      g.circle(cx, cy, R + 1.5).fill({ color: 0xfef3c7, alpha: 0.25 });
      g.circle(cx, cy, R).fill({ color: 0xfcd34d, alpha: 0.6 });
    } else {
      g.circle(cx, cy, R + 1.5).fill({ color: 0xe0e7ff, alpha: 0.18 });
      g.circle(cx, cy, R).fill({ color: 0xf1f5f9, alpha: 0.75 });
    }
  }

  // ── lâmpadas pendentes ────────────────────────────────────────
  // Uma ou duas lâmpadas penduradas no teto. À noite acendem (halo).
  private drawLamps(r: Room) {
    const doorW = 28;
    const doorMarginRight = 14;
    const doorX = r.x + r.width - doorMarginRight - doorW;

    // Lâmpadas dispostas na metade superior da sala, evitando a porta.
    const leftLim = r.x + 30;
    const rightLim = doorX - 20;
    const available = rightLim - leftLim;
    if (available < 40) return;

    const count = available >= 220 ? 2 : 1;
    const positions: number[] = [];
    if (count === 1) {
      positions.push(leftLim + available / 2);
    } else {
      positions.push(leftLim + available * 0.28);
      positions.push(leftLim + available * 0.72);
    }

    for (const lx of positions) {
      // Fio
      const fixture = new Graphics();
      const cordLen = 22;
      const ceilTop = r.ceilingY + 4;
      fixture.rect(lx - 0.5, ceilTop, 1, cordLen).fill({ color: 0x1e293b });
      // Canopla
      fixture.rect(lx - 3, ceilTop - 1, 6, 2).fill({ color: 0x334155 });

      // Cúpula (abajur cônico)
      const bulbY = ceilTop + cordLen;
      fixture
        .moveTo(lx - 7, bulbY)
        .lineTo(lx + 7, bulbY)
        .lineTo(lx + 5, bulbY + 6)
        .lineTo(lx - 5, bulbY + 6)
        .closePath()
        .fill({ color: 0x1f2937 })
        .stroke({ width: 1, color: 0x0f172a, alpha: 0.6 });
      fixture.rect(lx - 6, bulbY, 12, 1).fill({ color: 0x0f172a, alpha: 0.7 });

      // Feixe triangular estilo 2D game — lados retos, sem glow redondo.
      // Base larga, quase alcançando o chão, com alpha caindo em faixas
      // horizontais pra fugir do "triângulo sólido" duro.
      const halo = new Graphics();
      const coneTopY = bulbY + 6;
      const coneBotY = r.floorY - 6;
      const topHalf = 2;
      const outerBottomHalf = 110;
      const coneH = coneBotY - coneTopY;
      const bands = 12;
      for (let i = 0; i < bands; i++) {
        const t0 = i / bands;
        const t1 = (i + 1) / bands;
        const yTop = coneTopY + t0 * coneH;
        const yBot = coneTopY + t1 * coneH;
        const halfTop = topHalf + (outerBottomHalf - topHalf) * t0;
        const halfBot = topHalf + (outerBottomHalf - topHalf) * t1;
        const a = 0.15 * (1 - Math.pow(t1, 1.6));
        if (a <= 0.005) continue;
        halo
          .moveTo(lx - halfTop, yTop)
          .lineTo(lx + halfTop, yTop)
          .lineTo(lx + halfBot, yBot)
          .lineTo(lx - halfBot, yBot)
          .closePath()
          .fill({ color: 0xfef3c7, alpha: a });
      }
      halo.alpha = this.day.lampIntensity;

      const bulb = new Graphics();
      bulb.circle(lx, bulbY + 6, 3).fill({ color: 0xfde68a });
      bulb.circle(lx, bulbY + 6, 2).fill({ color: 0xfef9c3 });
      bulb.alpha = 0.25 + this.day.lampIntensity * 0.75;

      // Ordem: halo atrás (fica atrás da cúpula também), depois cúpula, depois bulbo.
      r.lampsContainer.addChild(halo);
      r.lampsContainer.addChild(fixture);
      r.lampsContainer.addChild(bulb);
      r.lamps.push({ fixture, bulb, halo });
    }
  }

  // Recalcula a hora e atualiza sky + astros + lâmpadas sem reconstruir salas.
  private refreshDaylight() {
    const prevPhase = this.day.phase;
    this.day = computeDaylight();
    // Sempre redesenha sol/lua (posição muda ao longo do dia).
    this.drawSun();
    this.drawMoon();
    // Sol/lua na janela mais próxima — recalcula a cada refresh.
    this.drawCelestialInWindows();
    // Se mudou a fase: atualiza sky, stars e tint.
    if (prevPhase !== this.day.phase) {
      this.drawSky();
      this.drawStars();
      this.drawNightTint();
      // Vidros das janelas dependem do céu — redesenhar as salas é custoso,
      // então só atualiza lâmpadas aqui e deixa vidros para o próximo relayout.
    } else {
      // Intensidade pode ter mudado no entardecer sem mudar de fase.
      this.drawNightTint();
    }
    // Lâmpadas acompanham a intensidade em tempo real.
    for (const r of this.rooms.values()) {
      for (const l of r.lamps) {
        l.halo.alpha = this.day.lampIntensity;
        l.bulb.alpha = 0.25 + this.day.lampIntensity * 0.75;
      }
    }
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
        const slots = computeSlots(n, r.width, r.x, r.floorY, r.ceilingY);
        r.animState.cursors = [];
        for (const [sx] of slots) {
          const g = new Graphics();
          g.rect(sx - 14, r.floorY - 58, 28, 30).fill({ color: 0x0f172a });
          g.rect(sx - 12, r.floorY - 56, 24, 26).fill({ color: 0x0ea5e9, alpha: 0.55 });
          for (let i = 0; i < 4; i++) {
            const lw = 4 + Math.random() * 16;
            g.rect(sx - 10, r.floorY - 54 + i * 4, lw, 1.4).fill({ color: 0x67e8f9 });
          }
          g.rect(sx - 3, r.floorY - 30, 6, 6).fill({ color: 0x334155 });
          g.rect(sx - 10, r.floorY - 25, 20, 2).fill({ color: 0x334155 });
          g.rect(sx - 22, r.floorY - 22, 44, 4).fill({ color: 0x57534e });
          g.rect(sx - 20, r.floorY - 18, 3, 14).fill({ color: 0x44403c });
          g.rect(sx + 17, r.floorY - 18, 3, 14).fill({ color: 0x44403c });
          g.rect(sx - 14, r.floorY - 24, 28, 2).fill({ color: 0x0f172a });
          d.addChild(g);
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
        const radar = new Graphics();
        radar.circle(cx, cy, 26).stroke({ width: 1.5, color: 0x475569, alpha: 0.75 });
        radar.circle(cx, cy, 18).stroke({ width: 1, color: 0x475569, alpha: 0.6 });
        radar.circle(cx, cy, 10).stroke({ width: 1, color: 0x475569, alpha: 0.55 });
        radar.moveTo(cx - 26, cy).lineTo(cx + 26, cy).stroke({ width: 0.8, color: 0x475569, alpha: 0.4 });
        radar.moveTo(cx, cy - 26).lineTo(cx, cy + 26).stroke({ width: 0.8, color: 0x475569, alpha: 0.4 });
        radar.circle(cx, cy, 2).fill({ color: 0x22c55e });
        d.addChild(radar);
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
        const dots = new Graphics();
        for (let i = 0; i < 5; i++) {
          const a = Math.random() * Math.PI * 2;
          const rr = 5 + Math.random() * 20;
          dots
            .circle(cx + Math.cos(a) * rr, cy + Math.sin(a) * rr, 1.2)
            .fill({ color: 0xef4444, alpha: 0.8 });
        }
        d.addChild(dots);
        break;
      }
      case 'desks': {
        const cx = r.x + r.width / 2;
        const g = new Graphics();
        g.rect(r.x + 12, inner.y1 + 2, 34, 24)
          .fill({ color: 0xf3f4f6 })
          .stroke({ width: 1, color: 0x57534e });
        g.moveTo(r.x + 16, inner.y1 + 20)
          .lineTo(r.x + 24, inner.y1 + 10)
          .lineTo(r.x + 32, inner.y1 + 18)
          .lineTo(r.x + 40, inner.y1 + 12)
          .stroke({ width: 1.2, color: 0x2563eb });
        g.rect(cx - 34, r.floorY - 22, 68, 4).fill({ color: 0x78716c });
        g.rect(cx - 32, r.floorY - 18, 3, 14).fill({ color: 0x57534e });
        g.rect(cx + 29, r.floorY - 18, 3, 14).fill({ color: 0x57534e });
        g.rect(cx - 26, r.floorY - 44, 2, 22).fill({ color: 0x0f172a });
        g.moveTo(cx - 28, r.floorY - 44)
          .lineTo(cx - 16, r.floorY - 44)
          .lineTo(cx - 18, r.floorY - 36)
          .lineTo(cx - 26, r.floorY - 36)
          .closePath()
          .fill({ color: 0xfef3c7 });
        g.rect(cx + 10, r.floorY - 36, 4, 14).fill({ color: 0xef4444 });
        g.rect(cx + 15, r.floorY - 36, 4, 14).fill({ color: 0x2563eb });
        g.rect(cx + 20, r.floorY - 34, 4, 12).fill({ color: 0x16a34a });
        g.rect(cx - 6, r.floorY - 30, 6, 8)
          .fill({ color: 0xffffff })
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
          for (let vx = 0; vx < rackW - 14; vx += 3) {
            g.rect(rackX + 6 + vx, yy + 2, 1, 2).fill({ color: 0x0f172a });
          }
        }
        d.addChild(g);
        r.animState.leds = [];
        for (let row = 0; row < 7; row++) {
          const led = new Graphics();
          led.circle(0, 0, 1.8).fill({ color: row === 2 ? 0xef4444 : 0x22c55e });
          led.x = rackX + rackW - 6;
          led.y = rackY + 8 + row * 9;
          d.addChild(led);
          r.animState.leds.push({ g: led, phase: Math.random() * Math.PI * 2 });
        }
        const cables = new Graphics();
        cables
          .moveTo(rackX, rackY + rackH - 4)
          .quadraticCurveTo(rackX - 10, r.floorY - 10, rackX - 4, r.floorY - 2)
          .stroke({ width: 2, color: 0x0f172a });
        cables
          .moveTo(rackX + rackW, rackY + rackH - 8)
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
        const panel = new Graphics();
        panel
          .roundRect(r.x + r.width - 42, r.floorY - 30, 34, 20, 3)
          .fill({ color: 0x0f172a });
        panel
          .circle(r.x + r.width - 30, r.floorY - 20, 6)
          .stroke({ width: 1, color: 0xfbbf24 });
        panel
          .moveTo(r.x + r.width - 30, r.floorY - 20)
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
            bg.moveTo(xx + 2, yy + cellH / 2)
              .lineTo(xx + cellW - 6, yy + cellH / 2)
              .stroke({ width: 0.4, color: 0x475569, alpha: 0.6 });
            d.addChild(bg);
            const graph = new Graphics();
            d.addChild(graph);
            r.animState.graphs!.push({
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
    // Reserva o canto inferior direito (porta + placa) — descontamos da largura útil
    // para que nenhum agente pise na placa.
    const DOOR_RESERVE = 46;
    const usableWidth = Math.max(this.MIN_ROOM_W / 2, r.width - DOOR_RESERVE);
    const slots = computeSlots(n, usableWidth, r.x, r.floorY, r.ceilingY);
    r.agents.forEach((name, i) => {
      this.positions.set(name, slots[i] || [r.x + r.width / 2, r.floorY - 30]);
    });
  }
}

// Reservado para a placa do departamento no teto + folga para a cabeça do sprite.
const DEPT_LABEL_RESERVE = 48;

// distribui N slots dentro da sala — em linha, com wrap para outra fileira se não couber.
// Respeita um topo mínimo para não colidir com a placa do departamento.
function computeSlots(
  n: number,
  width: number,
  startX: number,
  floorY: number,
  ceilingY: number,
): [number, number][] {
  const pad = 18;
  const slotW = 56;
  const rowH = 56;
  const maxCols = Math.max(1, Math.floor((width - pad * 2) / slotW));
  const minTopY = ceilingY + DEPT_LABEL_RESERVE;

  // Calcula cols/rows tentando caber 3 rows no máximo; se não couber, espreme colunas.
  let cols = Math.min(n, maxCols);
  let rows = Math.ceil(n / cols);
  const baseTop = (r: number) => floorY - 30 - (r - 1) * rowH;
  while (rows > 1 && baseTop(rows) < minTopY && cols < n) {
    cols = Math.min(n, cols + 1);
    rows = Math.ceil(n / cols);
  }

  const cellW = (width - pad * 2) / cols;
  const out: [number, number][] = [];
  for (let i = 0; i < n; i++) {
    const col = i % cols;
    const row = Math.floor(i / cols);
    const x = startX + pad + cellW * (col + 0.5);
    let y = floorY - 30 - (rows - 1 - row) * rowH;
    // Última garantia: nunca deixar um slot invadir a placa do departamento.
    if (y < minTopY) y = minTopY;
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
