import { Application, Container, Graphics, Text, TextStyle, Ticker } from 'pixi.js';
import type { AgentCfg, AgentType, AgentsData } from '@/lib/types';

const TYPE_COLORS: Record<AgentType, number> = {
  core: 0x6366f1,
  specialist: 0x0ea5e9,
  integrator: 0xa855f7,
  motor: 0xf97316,
  firefighter: 0xef4444,
};

const TYPE_EMOJI: Record<AgentType, string> = {
  core: '🧭',
  specialist: '🧠',
  integrator: '🔌',
  motor: '⚙️',
  firefighter: '🚒',
};

interface AgentNode {
  name: string;
  cfg: AgentCfg;
  container: Container;
  ring: Graphics;
  body: Graphics;
  led: Graphics;
  label: Text;
  workingLabel: Text;
  emoji: Text;
  targetX: number;
  targetY: number;
  x: number;
  y: number;
  vx: number;
  vy: number;
  working: boolean;
  pulseT: number;
}

export interface AgentCanvasOptions {
  onSelect?: (name: string) => void;
  selected?: string | null;
}

export class AgentCanvas {
  private app: Application;
  private nodes = new Map<string, AgentNode>();
  private links!: Graphics;
  private layer!: Container;
  private opts: AgentCanvasOptions;
  private selected: string | null = null;
  private ticker: ((t: Ticker) => void) | null = null;
  private width = 600;
  private height = 420;

  constructor(opts: AgentCanvasOptions = {}) {
    this.opts = opts;
    this.selected = opts.selected ?? null;
    this.app = new Application();
  }

  async mount(el: HTMLElement) {
    await this.app.init({
      resizeTo: el,
      background: 0xf8fafc,
      antialias: true,
      autoDensity: true,
      resolution: Math.min(window.devicePixelRatio, 2),
    });
    el.appendChild(this.app.canvas);
    this.width = el.clientWidth;
    this.height = el.clientHeight;

    this.links = new Graphics();
    this.layer = new Container();
    this.app.stage.addChild(this.links);
    this.app.stage.addChild(this.layer);

    this.ticker = (t: Ticker) => this.tick(t.deltaTime);
    this.app.ticker.add(this.ticker);
  }

  destroy() {
    if (this.ticker) this.app.ticker.remove(this.ticker);
    this.app.destroy(true, { children: true });
  }

  setSelected(name: string | null) {
    this.selected = name;
    for (const n of this.nodes.values()) {
      const isSel = n.name === name;
      n.container.scale.set(isSel ? 1.12 : 1);
    }
  }

  setWorking(name: string, label?: string) {
    const n = this.nodes.get(name);
    if (!n) return;
    n.working = true;
    n.workingLabel.text = label || 'trabalhando';
    n.workingLabel.visible = true;
  }

  clearWorking(name: string) {
    const n = this.nodes.get(name);
    if (!n) return;
    n.working = false;
    n.workingLabel.visible = false;
  }

  update(data: AgentsData) {
    const agents = data.manifest.agents;
    const names = Object.keys(agents);
    const existing = new Set(this.nodes.keys());
    const wanted = new Set(names);

    for (const n of existing) {
      if (!wanted.has(n)) {
        const node = this.nodes.get(n)!;
        this.layer.removeChild(node.container);
        node.container.destroy({ children: true });
        this.nodes.delete(n);
      }
    }

    const positions = this.layout(names, agents);
    for (const name of names) {
      const cfg = agents[name];
      let node = this.nodes.get(name);
      const [tx, ty] = positions[name];
      if (!node) {
        node = this.createNode(name, cfg, tx, ty);
        this.nodes.set(name, node);
        this.layer.addChild(node.container);
      } else {
        node.cfg = cfg;
        node.targetX = tx;
        node.targetY = ty;
        node.label.text = name;
        node.emoji.text = cfg.emoji || TYPE_EMOJI[(cfg.type || 'specialist') as AgentType];
        const color = TYPE_COLORS[(cfg.type || 'specialist') as AgentType] ?? 0x64748b;
        node.body.clear();
        node.body.circle(0, 0, 34).fill({ color, alpha: 0.14 }).stroke({ width: 2, color });
      }
    }

    this.drawLinks();
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

    const rings: Array<{ type: AgentType; r: number }> = [
      { type: 'core', r: 110 },
      { type: 'specialist', r: 150 },
      { type: 'integrator', r: 185 },
      { type: 'motor', r: 215 },
      { type: 'firefighter', r: 240 },
    ];

    for (const { type, r } of rings) {
      const list = (byType[type] || []).filter((n) => n !== manager);
      if (!list.length) continue;
      const step = (2 * Math.PI) / list.length;
      const phase = type === 'specialist' ? -Math.PI / 2 : 0;
      list.forEach((n, i) => {
        out[n] = [cx + r * Math.cos(phase + i * step), cy + r * Math.sin(phase + i * step)];
      });
    }

    for (const n of names) if (!out[n]) out[n] = [cx, cy];
    return out;
  }

  private createNode(name: string, cfg: AgentCfg, x: number, y: number): AgentNode {
    const container = new Container();
    container.x = x;
    container.y = y;
    container.eventMode = 'static';
    container.cursor = 'pointer';
    container.on('pointertap', () => this.opts.onSelect?.(name));

    const color = TYPE_COLORS[(cfg.type || 'specialist') as AgentType] ?? 0x64748b;

    const ring = new Graphics();
    ring.circle(0, 0, 42).stroke({ width: 2, color, alpha: 0.5 });
    ring.alpha = 0;
    container.addChild(ring);

    const body = new Graphics();
    body.circle(0, 0, 34).fill({ color, alpha: 0.14 }).stroke({ width: 2, color });
    container.addChild(body);

    const emoji = new Text({
      text: cfg.emoji || TYPE_EMOJI[(cfg.type || 'specialist') as AgentType],
      style: new TextStyle({ fontSize: 24, fill: 0x0f172a }),
    });
    emoji.anchor.set(0.5);
    container.addChild(emoji);

    const led = new Graphics();
    led.circle(0, 0, 5).fill({ color: 0x10b981 }).stroke({ width: 1, color: 0xffffff });
    led.position.set(24, -24);
    led.visible = false;
    container.addChild(led);

    const label = new Text({
      text: name,
      style: new TextStyle({
        fontSize: 12,
        fill: 0x1e293b,
        fontWeight: '600',
        fontFamily: 'ui-sans-serif, system-ui',
      }),
    });
    label.anchor.set(0.5, 0);
    label.position.set(0, 40);
    container.addChild(label);

    const workingLabel = new Text({
      text: 'trabalhando',
      style: new TextStyle({
        fontSize: 10,
        fill: 0x10b981,
        fontStyle: 'italic',
        fontFamily: 'ui-sans-serif, system-ui',
      }),
    });
    workingLabel.anchor.set(0.5, 0);
    workingLabel.position.set(0, 56);
    workingLabel.visible = false;
    container.addChild(workingLabel);

    return {
      name,
      cfg,
      container,
      ring,
      body,
      led,
      label,
      workingLabel,
      emoji,
      targetX: x,
      targetY: y,
      x,
      y,
      vx: 0,
      vy: 0,
      working: false,
      pulseT: Math.random() * Math.PI * 2,
    };
  }

  private drawLinks() {
    this.links.clear();
    for (const [name, node] of this.nodes) {
      const consults = node.cfg.consults || [];
      for (const target of consults) {
        const to = this.nodes.get(target);
        if (!to) continue;
        this.links
          .moveTo(node.x, node.y)
          .lineTo(to.x, to.y)
          .stroke({ width: 1, color: 0x94a3b8, alpha: 0.35 });
      }
      if (name === this.selected) {
        this.links.circle(node.x, node.y, 48).stroke({ width: 2, color: 0x2563eb, alpha: 0.4 });
      }
    }
  }

  private tick(delta: number) {
    for (const node of this.nodes.values()) {
      const dx = node.targetX - node.x;
      const dy = node.targetY - node.y;
      node.x += dx * 0.1 * delta;
      node.y += dy * 0.1 * delta;
      node.container.position.set(node.x, node.y);

      if (node.working) {
        node.pulseT += 0.08 * delta;
        const s = 1 + Math.sin(node.pulseT) * 0.04;
        node.body.scale.set(s);
        node.ring.alpha = 0.4 + Math.sin(node.pulseT) * 0.35;
        node.led.visible = Math.sin(node.pulseT * 3) > 0;
      } else {
        node.body.scale.set(1);
        node.ring.alpha = 0;
        node.led.visible = false;
      }
    }
    this.drawLinks();
  }
}
