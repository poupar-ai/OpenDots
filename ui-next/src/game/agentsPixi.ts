import { Application, Container, Graphics, Text, TextStyle, Ticker } from 'pixi.js';
import type { AgentCfg, AgentType, AgentsData } from '@/lib/types';
import { AgentSprite } from './sprites/AgentSprite';
import { Office, type DecorKind, type DepartmentInput } from './scene/Office';

interface AgentNode {
  name: string;
  cfg: AgentCfg;
  sprite: AgentSprite;
  targetX: number;
  targetY: number;
  x: number;
  y: number;
  vx: number; // velocidade (px/tick)
  vy: number;
  dragging?: boolean;
  pinned?: boolean; // posição definida pelo usuário; não recalcular pelo layout automático
}

interface DragState {
  name: string;
  offX: number; // offset do ponteiro em relação ao centro do sprite
  offY: number;
  moved: boolean;
  pointerId: number;
  lastX: number;
  lastY: number;
  lastT: number;
  vx: number; // velocidade acumulada do ponteiro, em px/frame
  vy: number;
}

// constantes físicas
const SPRING_K = 0.06;    // intensidade da mola pro alvo
const DAMPING = 0.82;     // amortecimento por frame
const REPEL_R = 40;       // raio de repulsão entre agentes
const REPEL_K = 0.08;     // intensidade da repulsão
const WALL_BOUNCE = 0.3;  // coeficiente de restituição nas paredes
const THROW_CAP = 22;     // velocidade máxima ao arremessar

interface FlowArrow {
  from: string;
  to: string;
  kind: 'out' | 'in';
  label: Text;
  labelBg: Graphics;
}

interface FlowDot {
  g: Graphics;
  from: string;
  to: string;
  t: number;
  speed: number;
  kind: 'out' | 'in';
}

interface PanState {
  startX: number;   // viewport
  startY: number;
  startCamX: number;
  startCamY: number;
  pointerId: number;
  moved: boolean;
}

export interface AgentCanvasOptions {
  onSelect?: (name: string) => void;
  selected?: string | null;
}

const MARGIN_X = 12;
const MARGIN_TOP = 80;    // abaixo do céu + telhado do mundo
const MARGIN_BOTTOM = 52; // acima do chão/calçada

// departamentos canônicos — cada um fica num andar do prédio (2D platform)
// andar 2 (topo): núcleo de comando; andar 1: devs + equipe; andar 0 (térreo):
// integrações, operações e monitoramento.
interface DeptSpec {
  id: string;
  label: string;
  decor: DecorKind;
  color: number;
  floor: number;
  match: (name: string, cfg: AgentCfg) => boolean;
}

const DEPARTMENTS: DeptSpec[] = [
  {
    id: 'core',
    label: 'Comando',
    decor: 'command',
    color: 0x6366f1,
    floor: 2,
    match: (_n, c) => (c.type || 'specialist') === 'core',
  },
  {
    id: 'devs',
    label: 'Devs',
    decor: 'computers',
    color: 0x16a34a,
    floor: 1,
    match: (_n, c) => c.profession === 'devs',
  },
  {
    id: 'specialists',
    label: 'Equipe',
    decor: 'desks',
    color: 0x0ea5e9,
    floor: 1,
    match: (_n, c) => (c.type || 'specialist') === 'specialist' && !c.profession,
  },
  {
    id: 'integrators',
    label: 'Integrações',
    decor: 'rack',
    color: 0xa855f7,
    floor: 0,
    match: (_n, c) => c.type === 'integrator',
  },
  {
    id: 'motors',
    label: 'Operações',
    decor: 'gears',
    color: 0xf97316,
    floor: 0,
    match: (_n, c) => c.type === 'motor',
  },
  {
    id: 'firefighter',
    label: 'Monitoramento',
    decor: 'monitors',
    color: 0xef4444,
    floor: 0,
    match: (_n, c) => c.type === 'firefighter',
  },
];

function edges(cfg: AgentCfg): string[] {
  return cfg.can_talk_to || cfg.consults || [];
}

export class AgentCanvas {
  private app: Application;
  private nodes = new Map<string, AgentNode>();
  private worldContainer!: Container;
  private baseLinks!: Graphics;
  private office!: Office;
  private flowLinks!: Graphics;
  private flowDotsLayer!: Container;
  private flowLabelsLayer!: Container;
  private layer!: Container;
  private opts: AgentCanvasOptions;
  private selected: string | null = null;
  private ticker: ((t: Ticker) => void) | null = null;
  private width = 600;
  private height = 420;
  private cameraX = 0;
  private cameraY = 0;
  private resizeObs: ResizeObserver | null = null;
  private workingName: string | null = null;
  private mounted = false;
  private destroyed = false;
  private pendingData: AgentsData | null = null;
  private flowArrows: FlowArrow[] = [];
  private flowDots: FlowDot[] = [];
  private flowPulse = 0;
  private drag: DragState | null = null;
  private pan: PanState | null = null;
  private wheelHandler: ((e: WheelEvent) => void) | null = null;
  private rootEl: HTMLElement | null = null;
  private readonly DRAG_THRESHOLD = 4;
  private readonly PAN_THRESHOLD = 3;

  constructor(opts: AgentCanvasOptions = {}) {
    this.opts = opts;
    this.selected = opts.selected ?? null;
    this.app = new Application();
  }

  async mount(el: HTMLElement) {
    if (this.destroyed) return;
    await this.app.init({
      resizeTo: el,
      background: 0xeaf4ff,
      antialias: true,
      autoDensity: true,
      resolution: Math.min(window.devicePixelRatio, 2),
    });
    if (this.destroyed) {
      try { this.app.destroy(true, { children: true }); } catch {}
      return;
    }
    el.appendChild(this.app.canvas);
    this.rootEl = el;
    this.width = el.clientWidth;
    this.height = el.clientHeight;

    this.worldContainer = new Container();
    this.office = new Office();
    this.baseLinks = new Graphics();
    this.flowLinks = new Graphics();
    this.flowDotsLayer = new Container();
    this.flowLabelsLayer = new Container();
    this.layer = new Container();

    // tudo que é desenhado em coordenadas de mundo fica dentro do worldContainer;
    // a câmera é só uma translação negativa (worldContainer.x = -cameraX).
    this.worldContainer.addChild(this.office);
    this.worldContainer.addChild(this.baseLinks);
    this.worldContainer.addChild(this.flowLinks);
    this.worldContainer.addChild(this.layer);
    this.worldContainer.addChild(this.flowDotsLayer);
    this.worldContainer.addChild(this.flowLabelsLayer);
    this.app.stage.addChild(this.worldContainer);

    this.office.resize(this.width, this.height);
    this.clampAndApplyCamera();

    // stage recebe os eventos de pointermove/up/down para suportar drag/pan fora do sprite
    this.app.stage.eventMode = 'static';
    this.app.stage.hitArea = this.app.screen;
    this.app.stage.on('pointerdown', (e) => this.onStagePointerDown(e.global.x, e.global.y, e.pointerId));
    this.app.stage.on('pointermove', (e) => this.onPointerMove(e.global.x, e.global.y));
    this.app.stage.on('pointerup', (e) => this.onPointerUp(e.global.x, e.global.y));
    this.app.stage.on('pointerupoutside', (e) => this.onPointerUp(e.global.x, e.global.y));

    // wheel: scroll horizontal + vertical pelo mundo
    this.wheelHandler = (e: WheelEvent) => this.onWheel(e);
    this.app.canvas.addEventListener('wheel', this.wheelHandler, { passive: false });

    this.ticker = (t: Ticker) => this.tick(t);
    this.app.ticker.add(this.ticker);

    this.resizeObs = new ResizeObserver(() => {
      if (!this.mounted || this.destroyed) return;
      this.width = el.clientWidth;
      this.height = el.clientHeight;
      this.office.resize(this.width, this.height);
      this.clampAndApplyCamera();
      this.relayout();
    });
    this.resizeObs.observe(el);

    this.mounted = true;
    if (this.pendingData) {
      const d = this.pendingData;
      this.pendingData = null;
      this.update(d);
    }
    for (const n of this.nodes.values()) n.sprite.setSelected(n.name === this.selected);
    if (this.workingName) this.nodes.get(this.workingName)?.sprite.setWorking(true);
    this.buildFlow();
  }

  destroy() {
    if (this.destroyed) return;
    this.destroyed = true;
    this.resizeObs?.disconnect();
    this.resizeObs = null;
    if (this.wheelHandler && this.app?.canvas) {
      try { this.app.canvas.removeEventListener('wheel', this.wheelHandler); } catch {}
    }
    this.wheelHandler = null;
    this.rootEl = null;
    if (!this.mounted) return;
    try { if (this.ticker) this.app.ticker.remove(this.ticker); } catch {}
    try { this.app.destroy(true, { children: true }); } catch {}
    this.mounted = false;
  }

  setSelected(name: string | null) {
    this.selected = name;
    if (!this.mounted) return;
    for (const n of this.nodes.values()) n.sprite.setSelected(n.name === name);
    this.applyAlpha();
    this.buildFlow();
    this.sortLayer();
  }

  setWorking(name: string | null) {
    if (this.workingName === name) return;
    if (!this.mounted) { this.workingName = name; return; }
    if (this.workingName) this.nodes.get(this.workingName)?.sprite.setWorking(false);
    this.workingName = name;
    if (name) this.nodes.get(name)?.sprite.setWorking(true);
  }

  update(data: AgentsData) {
    if (!this.mounted) { this.pendingData = data; return; }
    if (this.destroyed) return;
    const agents = data.manifest.agents;
    const names = Object.keys(agents);
    const wanted = new Set(names);

    for (const name of [...this.nodes.keys()]) {
      if (!wanted.has(name)) {
        const node = this.nodes.get(name)!;
        this.layer.removeChild(node.sprite);
        node.sprite.destroy({ children: true });
        this.nodes.delete(name);
      }
    }

    const positions = this.layout(names, agents);
    for (const name of names) {
      const cfg = agents[name];
      const type = (cfg.type || 'specialist') as AgentType;
      let node = this.nodes.get(name);
      const [tx, ty] = positions[name];
      if (!node) {
        const sprite = new AgentSprite(name, type, cfg.emoji, cfg.profession);
        sprite.x = tx;
        sprite.y = ty;
        sprite.on('pointerdown', (e) => {
          e.stopPropagation();
          this.startDrag(name, e.global.x, e.global.y, e.pointerId);
        });
        node = { name, cfg, sprite, targetX: tx, targetY: ty, x: tx, y: ty, vx: 0, vy: 0 };
        this.nodes.set(name, node);
        this.layer.addChild(sprite);
      } else {
        node.cfg = cfg;
        if (!node.pinned) {
          node.targetX = tx;
          node.targetY = ty;
        }
        node.sprite.updateName(name);
        node.sprite.updateType(type, cfg.profession);
      }
      // status do agente: último evento é erro → alert + balão; caso contrário limpa.
      const log = data.logs?.[name];
      const isCurrentlyFailing = log?.lastEventType === 'error';
      if (isCurrentlyFailing) {
        node!.sprite.setStatus('alert');
        node!.sprite.setErrorTooltip(log?.lastErrorMsg ?? null, log?.lastErrorHint ?? null);
      } else {
        if (type === 'firefighter') node!.sprite.setStatus('calm');
        else node!.sprite.setStatus('none');
        node!.sprite.setErrorTooltip(null, null);
      }
    }

    this.applyAlpha();
    this.buildFlow();
    this.sortLayer();
  }

  private applyAlpha() {
    if (!this.selected) {
      for (const n of this.nodes.values()) n.sprite.alpha = 1;
      return;
    }
    const sel = this.nodes.get(this.selected);
    const related = new Set<string>();
    if (sel) {
      related.add(this.selected);
      for (const n of edges(sel.cfg)) related.add(n);
      for (const [name, node] of this.nodes) {
        if (edges(node.cfg).includes(this.selected)) related.add(name);
      }
    }
    for (const [name, node] of this.nodes) {
      node.sprite.alpha = related.has(name) ? 1 : 0.28;
    }
  }

  private sortLayer() {
    const list = [...this.nodes.values()].sort((a, b) => {
      if (a.name === this.selected) return 1;
      if (b.name === this.selected) return -1;
      return a.y - b.y;
    });
    for (const n of list) this.layer.addChild(n.sprite);
  }

  private relayout() {
    if (!this.nodes.size) return;
    const names = [...this.nodes.keys()];
    const cfgs: Record<string, AgentCfg> = {};
    for (const [n, node] of this.nodes) cfgs[n] = node.cfg;
    const pos = this.layout(names, cfgs);
    for (const n of names) {
      const node = this.nodes.get(n)!;
      if (node.pinned) continue;
      [node.targetX, node.targetY] = pos[n];
    }
  }

  private layout(
    names: string[],
    agents: Record<string, AgentCfg>,
  ): Record<string, [number, number]> {
    // agrupa agentes em departamentos (ordem canônica)
    const depts: DepartmentInput[] = [];
    for (const spec of DEPARTMENTS) {
      const members = names.filter((n) => spec.match(n, agents[n]));
      if (!members.length) continue;
      // manager sempre primeiro dentro do core; depois, ordem por nome
      members.sort((a, b) => {
        if (a === 'manager') return -1;
        if (b === 'manager') return 1;
        return a.localeCompare(b);
      });
      depts.push({
        id: spec.id,
        label: spec.label,
        decor: spec.decor,
        color: spec.color,
        floor: spec.floor,
        agents: members,
      });
    }
    this.office.setDepartments(depts);
    this.clampAndApplyCamera();

    const out: Record<string, [number, number]> = {};
    for (const n of names) {
      const p = this.office.positionFor(n);
      out[n] = p || [this.width / 2, this.height / 2];
    }
    return out;
  }

  private buildFlow() {
    // limpar flow antigo
    for (const a of this.flowArrows) {
      this.flowLabelsLayer.removeChild(a.label);
      this.flowLabelsLayer.removeChild(a.labelBg);
      a.label.destroy();
      a.labelBg.destroy();
    }
    this.flowArrows = [];
    for (const d of this.flowDots) {
      this.flowDotsLayer.removeChild(d.g);
      d.g.destroy();
    }
    this.flowDots = [];
    this.flowLinks.clear();

    if (!this.selected) return;
    const sel = this.nodes.get(this.selected);
    if (!sel) return;

    // outgoing: selected → target
    for (const target of edges(sel.cfg)) {
      const to = this.nodes.get(target);
      if (!to) continue;
      this.addFlow(this.selected, target, 'out');
    }
    // incoming: other → selected
    for (const [name, node] of this.nodes) {
      if (name === this.selected) continue;
      if (edges(node.cfg).includes(this.selected)) {
        this.addFlow(name, this.selected, 'in');
      }
    }
  }

  private addFlow(from: string, to: string, kind: 'out' | 'in') {
    // label
    const text = kind === 'out' ? 'fala com' : 'reporta';
    const color = kind === 'out' ? 0xf97316 : 0x2563eb;
    const label = new Text({
      text,
      style: new TextStyle({
        fontSize: 10,
        fill: 0xffffff,
        fontWeight: '600',
        fontFamily: 'ui-sans-serif, system-ui',
      }),
    });
    label.anchor.set(0.5);
    const labelBg = new Graphics();
    this.flowLabelsLayer.addChild(labelBg);
    this.flowLabelsLayer.addChild(label);
    this.flowArrows.push({ from, to, kind, label, labelBg });

    // 4 partículas viajando pela linha
    for (let i = 0; i < 4; i++) {
      const g = new Graphics();
      g.circle(0, 0, 3.5).fill({ color }).stroke({ width: 1, color: 0xffffff, alpha: 0.9 });
      this.flowDotsLayer.addChild(g);
      this.flowDots.push({ g, from, to, t: i / 4, speed: 0.009, kind });
    }
  }

  private startDrag(name: string, gx: number, gy: number, pointerId: number) {
    const node = this.nodes.get(name);
    if (!node) return;
    const wx = gx + this.cameraX;
    const wy = gy + this.cameraY;
    this.drag = {
      name,
      offX: node.x - wx,
      offY: node.y - wy,
      moved: false,
      pointerId,
      lastX: node.x,
      lastY: node.y,
      lastT: performance.now(),
      vx: 0,
      vy: 0,
    };
    node.vx = 0;
    node.vy = 0;
    node.dragging = true;
    this.layer.addChild(node.sprite); // leva pro topo
    node.sprite.cursor = 'grabbing';
  }

  private onStagePointerDown(gx: number, gy: number, pointerId: number) {
    // se já estamos arrastando um sprite (sprite fez stopPropagation), ignora
    if (this.drag) return;
    this.pan = {
      startX: gx,
      startY: gy,
      startCamX: this.cameraX,
      startCamY: this.cameraY,
      pointerId,
      moved: false,
    };
  }

  private onPointerMove(gx: number, gy: number) {
    if (this.drag) {
      this.onDragMove(gx, gy);
      return;
    }
    if (this.pan) {
      this.onPanMove(gx, gy);
      return;
    }
  }

  private onDragMove(gx: number, gy: number) {
    if (!this.drag) return;
    const node = this.nodes.get(this.drag.name);
    if (!node) return;
    const { width: worldW, height: worldH } = this.office.getWorldSize();
    const wx = gx + this.cameraX;
    const wy = gy + this.cameraY;
    const nx = clamp(wx + this.drag.offX, MARGIN_X, worldW - MARGIN_X);
    const ny = clamp(wy + this.drag.offY, MARGIN_TOP, worldH - MARGIN_BOTTOM);
    if (!this.drag.moved) {
      const dx = nx - node.x;
      const dy = ny - node.y;
      if (Math.hypot(dx, dy) >= this.DRAG_THRESHOLD) this.drag.moved = true;
    }
    const now = performance.now();
    const dt = Math.max(1, now - this.drag.lastT);
    const frames = dt / 16.666;
    const vxp = (nx - this.drag.lastX) / frames;
    const vyp = (ny - this.drag.lastY) / frames;
    this.drag.vx = this.drag.vx * 0.4 + vxp * 0.6;
    this.drag.vy = this.drag.vy * 0.4 + vyp * 0.6;
    this.drag.lastX = nx;
    this.drag.lastY = ny;
    this.drag.lastT = now;

    node.x = nx;
    node.y = ny;
    node.targetX = nx;
    node.targetY = ny;
    node.sprite.position.set(nx, ny);
  }

  private onPanMove(gx: number, gy: number) {
    if (!this.pan) return;
    const dx = gx - this.pan.startX;
    const dy = gy - this.pan.startY;
    if (!this.pan.moved && Math.hypot(dx, dy) >= this.PAN_THRESHOLD) this.pan.moved = true;
    // arrastar ponteiro p/ direita → puxa mundo p/ direita → câmera anda p/ esquerda
    this.cameraX = this.pan.startCamX - dx;
    this.cameraY = this.pan.startCamY - dy;
    this.clampAndApplyCamera();
  }

  private onPointerUp(_gx: number, _gy: number) {
    if (this.drag) {
      const { name, moved, vx, vy } = this.drag;
      this.drag = null;
      const node = this.nodes.get(name);
      if (node) {
        node.dragging = false;
        node.sprite.cursor = 'pointer';
      }
      if (!moved && node) {
        node.sprite.greet();
        this.opts.onSelect?.(name);
      } else if (moved && node) {
        node.pinned = true;
        node.vx = clamp(vx, -THROW_CAP, THROW_CAP);
        node.vy = clamp(vy, -THROW_CAP, THROW_CAP);
      }
      return;
    }
    if (this.pan) {
      this.pan = null;
    }
  }

  private onWheel(e: WheelEvent) {
    const { width: worldW, height: worldH } = this.office.getWorldSize();
    const maxX = Math.max(0, worldW - this.width);
    const maxY = Math.max(0, worldH - this.height);
    if (maxX === 0 && maxY === 0) return;
    e.preventDefault();
    // shift inverte o eixo — mouse sem roda horizontal ainda consegue pan lateral
    const dx = e.shiftKey ? e.deltaY : e.deltaX;
    const dy = e.shiftKey ? 0 : e.deltaY;
    this.cameraX += dx;
    this.cameraY += dy;
    this.clampAndApplyCamera();
  }

  private clampAndApplyCamera() {
    if (!this.office) return;
    const { width: worldW, height: worldH } = this.office.getWorldSize();
    const maxX = Math.max(0, worldW - this.width);
    const maxY = Math.max(0, worldH - this.height);
    this.cameraX = clamp(this.cameraX, 0, maxX);
    this.cameraY = clamp(this.cameraY, 0, maxY);
    this.worldContainer.x = -this.cameraX;
    this.worldContainer.y = -this.cameraY;
  }

  private tick(t: Ticker) {
    const dt = Math.min(t.deltaTime, 2);
    this.office?.tick(t);

    const nodes = [...this.nodes.values()];

    // 1) mola + amortecimento em direção ao target (pinados usam targetX/Y do release)
    for (const node of nodes) {
      if (node.dragging) continue;
      node.vx += (node.targetX - node.x) * SPRING_K * dt;
      node.vy += (node.targetY - node.y) * SPRING_K * dt;
      const d = Math.pow(DAMPING, dt);
      node.vx *= d;
      node.vy *= d;
    }

    // 2) repulsão suave entre agentes
    for (let i = 0; i < nodes.length; i++) {
      const a = nodes[i];
      for (let j = i + 1; j < nodes.length; j++) {
        const b = nodes[j];
        const dx = b.x - a.x;
        const dy = b.y - a.y;
        const d = Math.hypot(dx, dy) || 0.001;
        if (d < REPEL_R) {
          const overlap = REPEL_R - d;
          const nx = dx / d;
          const ny = dy / d;
          const push = overlap * REPEL_K * dt;
          if (!a.dragging) { a.vx -= nx * push; a.vy -= ny * push; }
          if (!b.dragging) { b.vx += nx * push; b.vy += ny * push; }
        }
      }
    }

    // 3) integra + colisão com paredes da sala (ou canvas se pinado)
    for (const node of nodes) {
      if (node.dragging) {
        node.sprite.tick(t);
        continue;
      }
      node.x += node.vx * dt;
      node.y += node.vy * dt;

      const bounds = node.pinned ? null : this.office.getRoomBounds(node.name);
      const world = this.office.getWorldSize();
      const minX = bounds ? bounds.x1 : MARGIN_X;
      const maxX = bounds ? bounds.x2 : world.width - MARGIN_X;
      const minY = bounds ? bounds.ceilingY + 40 : MARGIN_TOP;
      const maxY = bounds ? bounds.floorY - 30 : world.height - MARGIN_BOTTOM;
      if (node.x < minX) { node.x = minX; node.vx = Math.abs(node.vx) * WALL_BOUNCE; }
      if (node.x > maxX) { node.x = maxX; node.vx = -Math.abs(node.vx) * WALL_BOUNCE; }
      if (node.y < minY) { node.y = minY; node.vy = Math.abs(node.vy) * WALL_BOUNCE; }
      if (node.y > maxY) { node.y = maxY; node.vy = -Math.abs(node.vy) * WALL_BOUNCE; }

      node.sprite.position.set(node.x, node.y);
      node.sprite.tick(t);
    }

    this.drawBaseLinks();
    this.drawFlow(dt);
  }

  private drawBaseLinks() {
    this.baseLinks.clear();
    const alphaDim = this.selected ? 0.08 : 0.28;
    const seen = new Set<string>();
    for (const [name, node] of this.nodes) {
      for (const target of edges(node.cfg)) {
        const to = this.nodes.get(target);
        if (!to) continue;
        const key = [name, target].sort().join('|');
        if (seen.has(key)) continue;
        seen.add(key);
        const mx = (node.x + to.x) / 2;
        const my = (node.y + to.y) / 2 + 10;
        this.baseLinks
          .moveTo(node.x, node.y + 10)
          .quadraticCurveTo(mx, my, to.x, to.y + 10)
          .stroke({ width: 1.2, color: 0x94a3b8, alpha: alphaDim });
      }
    }
  }

  private drawFlow(dt: number) {
    this.flowLinks.clear();
    if (!this.selected) return;
    this.flowPulse += 0.08 * dt;

    for (const a of this.flowArrows) {
      const from = this.nodes.get(a.from);
      const to = this.nodes.get(a.to);
      if (!from || !to) continue;

      const fx = from.x;
      const fy = from.y + 6;
      const tx = to.x;
      const ty = to.y + 6;
      const dx = tx - fx;
      const dy = ty - fy;
      const dist = Math.hypot(dx, dy) || 1;
      // recua as pontas pro sprite
      const pad = 30;
      const sx = fx + (dx / dist) * pad;
      const sy = fy + (dy / dist) * pad;
      const ex = tx - (dx / dist) * pad;
      const ey = ty - (dy / dist) * pad;

      const color = a.kind === 'out' ? 0xf97316 : 0x2563eb;
      const pulseAlpha = 0.55 + Math.sin(this.flowPulse) * 0.2;

      this.flowLinks
        .moveTo(sx, sy)
        .lineTo(ex, ey)
        .stroke({ width: 2.2, color, alpha: pulseAlpha });

      // seta (arrowhead)
      const ang = Math.atan2(dy, dx);
      const ah = 8;
      const ax1 = ex - Math.cos(ang - 0.5) * ah;
      const ay1 = ey - Math.sin(ang - 0.5) * ah;
      const ax2 = ex - Math.cos(ang + 0.5) * ah;
      const ay2 = ey - Math.sin(ang + 0.5) * ah;
      this.flowLinks
        .moveTo(ex, ey)
        .lineTo(ax1, ay1)
        .lineTo(ax2, ay2)
        .closePath()
        .fill({ color, alpha: pulseAlpha });

      // label no meio
      const mx = (sx + ex) / 2;
      const my = (sy + ey) / 2;
      a.label.x = mx;
      a.label.y = my;
      a.labelBg.clear();
      const w = a.label.width + 10;
      const h = 14;
      a.labelBg
        .roundRect(mx - w / 2, my - h / 2, w, h, 7)
        .fill({ color, alpha: 0.92 })
        .stroke({ width: 1, color: 0xffffff, alpha: 0.9 });
    }

    // atualiza dots
    for (const dot of this.flowDots) {
      dot.t += dot.speed * dt;
      if (dot.t > 1) dot.t -= 1;
      const from = this.nodes.get(dot.from);
      const to = this.nodes.get(dot.to);
      if (!from || !to) continue;
      const fx = from.x;
      const fy = from.y + 6;
      const tx = to.x;
      const ty = to.y + 6;
      const dx = tx - fx;
      const dy = ty - fy;
      const dist = Math.hypot(dx, dy) || 1;
      const pad = 30;
      const sx = fx + (dx / dist) * pad;
      const sy = fy + (dy / dist) * pad;
      const ex = tx - (dx / dist) * pad;
      const ey = ty - (dy / dist) * pad;
      dot.g.x = sx + (ex - sx) * dot.t;
      dot.g.y = sy + (ey - sy) * dot.t;
      dot.g.alpha = 1 - Math.abs(dot.t - 0.5) * 0.6;
    }
  }
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, v));
}
