import { Application, Container, Graphics, Text, TextStyle, Ticker } from 'pixi.js';
import type { AgentCfg, AgentType, AgentsData } from '@/lib/types';
import { AgentSprite } from './sprites/AgentSprite';
import { Backdrop } from './scene/Backdrop';

interface AgentNode {
  name: string;
  cfg: AgentCfg;
  sprite: AgentSprite;
  targetX: number;
  targetY: number;
  x: number;
  y: number;
  dragging?: boolean;
  pinned?: boolean; // posição definida pelo usuário; não recalcular pelo layout automático
}

interface DragState {
  name: string;
  offX: number; // offset do ponteiro em relação ao centro do sprite
  offY: number;
  moved: boolean;
  pointerId: number;
}

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

export interface AgentCanvasOptions {
  onSelect?: (name: string) => void;
  selected?: string | null;
}

const SPRITE_MIN_DIST = 105; // distância mínima entre centros
const MARGIN_X = 60;
const MARGIN_TOP = 50;
const MARGIN_BOTTOM = 80;

function edges(cfg: AgentCfg): string[] {
  return cfg.can_talk_to || cfg.consults || [];
}

export class AgentCanvas {
  private app: Application;
  private nodes = new Map<string, AgentNode>();
  private baseLinks!: Graphics;
  private backdrop!: Backdrop;
  private flowLinks!: Graphics;
  private flowDotsLayer!: Container;
  private flowLabelsLayer!: Container;
  private layer!: Container;
  private opts: AgentCanvasOptions;
  private selected: string | null = null;
  private ticker: ((t: Ticker) => void) | null = null;
  private width = 600;
  private height = 420;
  private resizeObs: ResizeObserver | null = null;
  private workingName: string | null = null;
  private mounted = false;
  private destroyed = false;
  private pendingData: AgentsData | null = null;
  private flowArrows: FlowArrow[] = [];
  private flowDots: FlowDot[] = [];
  private flowPulse = 0;
  private drag: DragState | null = null;
  private readonly DRAG_THRESHOLD = 4;

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
    this.width = el.clientWidth;
    this.height = el.clientHeight;

    this.backdrop = new Backdrop();
    this.baseLinks = new Graphics();
    this.flowLinks = new Graphics();
    this.flowDotsLayer = new Container();
    this.flowLabelsLayer = new Container();
    this.layer = new Container();

    this.app.stage.addChild(this.backdrop);
    this.app.stage.addChild(this.baseLinks);
    this.app.stage.addChild(this.flowLinks);
    this.app.stage.addChild(this.layer);
    this.app.stage.addChild(this.flowDotsLayer);
    this.app.stage.addChild(this.flowLabelsLayer);

    this.backdrop.resize(this.width, this.height);

    // stage recebe os eventos de pointermove/up para suportar drag fora do sprite
    this.app.stage.eventMode = 'static';
    this.app.stage.hitArea = this.app.screen;
    this.app.stage.on('pointermove', (e) => this.onPointerMove(e.global.x, e.global.y));
    this.app.stage.on('pointerup', (e) => this.onPointerUp(e.global.x, e.global.y));
    this.app.stage.on('pointerupoutside', (e) => this.onPointerUp(e.global.x, e.global.y));

    this.ticker = (t: Ticker) => this.tick(t);
    this.app.ticker.add(this.ticker);

    this.resizeObs = new ResizeObserver(() => {
      if (!this.mounted || this.destroyed) return;
      this.width = el.clientWidth;
      this.height = el.clientHeight;
      this.backdrop.resize(this.width, this.height);
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
        const sprite = new AgentSprite(name, type, cfg.emoji);
        sprite.x = tx;
        sprite.y = ty;
        sprite.on('pointerdown', (e) => {
          e.stopPropagation();
          this.startDrag(name, e.global.x, e.global.y, e.pointerId);
        });
        node = { name, cfg, sprite, targetX: tx, targetY: ty, x: tx, y: ty };
        this.nodes.set(name, node);
        this.layer.addChild(sprite);
      } else {
        node.cfg = cfg;
        if (!node.pinned) {
          node.targetX = tx;
          node.targetY = ty;
        }
        node.sprite.updateName(name);
        node.sprite.updateType(type);
      }
      // bombeiro fica calmo (sinal verde) por padrão;
      // o `!` vermelho só aparece quando houver sinal externo de problema.
      if (type === 'firefighter') node!.sprite.setStatus('calm');
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
    const cx = this.width / 2;
    const cy = this.height / 2;
    const out: Record<string, [number, number]> = {};
    const byType: Record<string, string[]> = {};
    for (const n of names) {
      const t = (agents[n].type || 'specialist') as AgentType;
      (byType[t] ||= []).push(n);
    }

    const manager = names.find((n) => n === 'manager') || (byType.core || [])[0];
    if (manager) out[manager] = [cx, cy];

    // raio mínimo pra caber N itens sem sobrepor
    const minR = (count: number, base: number) =>
      Math.max(base, (SPRITE_MIN_DIST * Math.max(count, 2)) / (2 * Math.PI));

    const maxRadius = Math.min(this.width, this.height) / 2 - 80;
    const cap = (r: number) => Math.min(r, maxRadius);

    const rings: Array<{ type: AgentType; base: number }> = [
      { type: 'core', base: 150 },
      { type: 'specialist', base: 170 },
      { type: 'integrator', base: 220 },
      { type: 'motor', base: 250 },
      { type: 'firefighter', base: 280 },
    ];

    for (const { type, base } of rings) {
      const list = (byType[type] || []).filter((n) => n !== manager);
      if (!list.length) continue;
      const r = cap(minR(list.length, base));
      const step = (2 * Math.PI) / list.length;
      const phase = -Math.PI / 2 + (type === 'specialist' ? 0 : step / 2);
      list.forEach((n, i) => {
        out[n] = [cx + r * Math.cos(phase + i * step), cy + r * Math.sin(phase + i * step)];
      });
    }
    for (const n of names) if (!out[n]) out[n] = [cx, cy];

    // colisão: empurra pares próximos
    for (let iter = 0; iter < 80; iter++) {
      let moved = false;
      for (let i = 0; i < names.length; i++) {
        for (let j = i + 1; j < names.length; j++) {
          const a = out[names[i]];
          const b = out[names[j]];
          const dx = b[0] - a[0];
          const dy = b[1] - a[1];
          const d = Math.hypot(dx, dy) || 0.001;
          if (d < SPRITE_MIN_DIST) {
            const push = (SPRITE_MIN_DIST - d) / 2 + 0.5;
            const nx = dx / d;
            const ny = dy / d;
            // manager não se mexe; bombeiros resistem menos
            if (names[i] !== manager) { a[0] -= nx * push; a[1] -= ny * push; }
            if (names[j] !== manager) { b[0] += nx * push; b[1] += ny * push; }
            moved = true;
          }
        }
      }
      if (!moved) break;
    }

    // clip no viewport
    for (const n of names) {
      out[n][0] = Math.max(MARGIN_X, Math.min(this.width - MARGIN_X, out[n][0]));
      out[n][1] = Math.max(MARGIN_TOP, Math.min(this.height - MARGIN_BOTTOM, out[n][1]));
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
    this.drag = { name, offX: node.x - gx, offY: node.y - gy, moved: false, pointerId };
    node.dragging = true;
    this.layer.addChild(node.sprite); // leva pro topo
    node.sprite.cursor = 'grabbing';
  }

  private onPointerMove(gx: number, gy: number) {
    if (!this.drag) return;
    const node = this.nodes.get(this.drag.name);
    if (!node) return;
    const nx = clamp(gx + this.drag.offX, MARGIN_X, this.width - MARGIN_X);
    const ny = clamp(gy + this.drag.offY, MARGIN_TOP, this.height - MARGIN_BOTTOM);
    if (!this.drag.moved) {
      const dx = nx - node.x;
      const dy = ny - node.y;
      if (Math.hypot(dx, dy) >= this.DRAG_THRESHOLD) this.drag.moved = true;
    }
    node.x = nx;
    node.y = ny;
    node.targetX = nx;
    node.targetY = ny;
    node.sprite.position.set(nx, ny);
  }

  private onPointerUp(_gx: number, _gy: number) {
    if (!this.drag) return;
    const { name, moved } = this.drag;
    const node = this.nodes.get(name);
    this.drag = null;
    if (node) {
      node.dragging = false;
      node.sprite.cursor = 'pointer';
    }
    if (!moved && node) {
      node.sprite.greet();
      this.opts.onSelect?.(name);
    } else if (moved && node) {
      node.pinned = true;
    }
  }

  private tick(t: Ticker) {
    const dt = t.deltaTime;
    this.backdrop?.tick(t);
    for (const node of this.nodes.values()) {
      if (!node.dragging) {
        const dx = node.targetX - node.x;
        const dy = node.targetY - node.y;
        node.x += dx * 0.12 * dt;
        node.y += dy * 0.12 * dt;
        node.sprite.position.set(node.x, node.y);
      }
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
