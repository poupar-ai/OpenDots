import { Container, Graphics, Text, TextStyle, Ticker } from 'pixi.js';
import type { AgentType } from '@/lib/types';

const PALETTE: Record<AgentType, { primary: number; accent: number; gear: number }> = {
  core:        { primary: 0x6366f1, accent: 0x312e81, gear: 0xfbbf24 },
  specialist:  { primary: 0x0ea5e9, accent: 0x0c4a6e, gear: 0xfef3c7 },
  integrator:  { primary: 0xa855f7, accent: 0x581c87, gear: 0x22d3ee },
  motor:       { primary: 0xf97316, accent: 0x7c2d12, gear: 0x94a3b8 },
  firefighter: { primary: 0xef4444, accent: 0x7f1d1d, gear: 0xfde047 },
};

// Cor da "camisa" por profissão — sobrescreve a palette do tipo
// quando o agente pertence a uma profissão conhecida.
const PROFESSION_PALETTE: Record<string, { primary: number; accent: number; gear: number }> = {
  devs: { primary: 0x16a34a, accent: 0x14532d, gear: 0xbbf7d0 },
};

function paletteFor(type: AgentType, profession?: string) {
  if (profession && PROFESSION_PALETTE[profession]) return PROFESSION_PALETTE[profession];
  return PALETTE[type] || PALETTE.specialist;
}

const SKIN = 0xfcd9b6;
const SHADOW = 0x0f172a;
const OUTLINE = 0x0f172a;

export type AgentState = 'idle' | 'walk' | 'talk' | 'work' | 'happy' | 'alert';

// Prioridade inspirada no Spine Boy (jump > hover > run > walk > idle):
// quanto maior o valor, mais tem precedência ao resolver estados concorrentes.
const STATE_PRIORITY: Record<AgentState, number> = {
  idle: 0,
  walk: 1,
  talk: 2,
  happy: 3,
  work: 4,
  alert: 5,
};

interface Particle {
  g: Graphics;
  vx: number;
  vy: number;
  life: number;
  max: number;
}

export class AgentSprite extends Container {
  public name: string;
  public type: AgentType;
  public profession: string | undefined;
  private palette: (typeof PALETTE)[AgentType];
  private directionalView: Container; // wrapper pra flip horizontal (padrão Spine Boy)
  private body: Container;
  private head: Container;
  private eyeL: Graphics;
  private eyeR: Graphics;
  private mouth: Graphics;
  private armL: Graphics;
  private armR: Graphics;
  private legL: Graphics;
  private legR: Graphics;
  private accessory: Container;
  private shadow: Graphics;
  private badge: Text;
  private badgeBg: Graphics;
  private statusMark: Text | null = null;
  private statusBg: Graphics | null = null;
  private statusKind: 'none' | 'calm' | 'alert' = 'none';
  private errorBubble: Container | null = null;
  private errorBubbleKey: string | null = null;
  private particles: Particle[] = [];
  private particleLayer: Container;
  private t = 0;
  private blinkT = 0;
  private blinkClose = 0;
  private workingKind: 'default' | 'fire' | 'gear' | 'plug' | 'sparkle' | 'compass' = 'default';

  private state: AgentState = 'idle';
  private prevState: AgentState = 'idle';
  private mix = 1; // 0..1 — 0 = começou a transição, 1 = totalmente no novo estado
  private readonly defaultMix = 0.2; // segundos (inspirado no defaultMix do Spine Boy)
  private stateTimer = 0; // tempo (s) no estado atual — usado para auto-voltar de happy
  private animationMap: Record<AgentState, (dt: number) => void>;

  constructor(name: string, type: AgentType, emoji?: string, profession?: string) {
    super();
    this.name = name;
    this.type = type;
    this.profession = profession;
    this.palette = paletteFor(type, profession);
    this.workingKind = {
      core: 'compass',
      specialist: 'sparkle',
      integrator: 'plug',
      motor: 'gear',
      firefighter: 'fire',
    }[type] as any;

    this.shadow = new Graphics();
    this.shadow.ellipse(0, 30, 22, 5).fill({ color: SHADOW, alpha: 0.18 });
    this.addChild(this.shadow);

    this.directionalView = new Container();
    this.addChild(this.directionalView);

    this.particleLayer = new Container();
    this.directionalView.addChild(this.particleLayer);

    this.body = new Container();
    this.directionalView.addChild(this.body);

    this.legL = this.makeLeg(-6);
    this.legR = this.makeLeg(6);
    this.body.addChild(this.legL);
    this.body.addChild(this.legR);

    const torso = new Graphics();
    torso
      .roundRect(-11, -8, 22, 22, 6)
      .fill({ color: this.palette.primary })
      .stroke({ width: 1.5, color: OUTLINE, alpha: 0.35 });
    torso.rect(-11, 2, 22, 3).fill({ color: this.palette.accent, alpha: 0.7 });
    this.body.addChild(torso);

    this.armL = this.makeArm(-11);
    this.armR = this.makeArm(11);
    this.body.addChild(this.armL);
    this.body.addChild(this.armR);

    this.head = new Container();
    const face = new Graphics();
    face.circle(0, -18, 11).fill({ color: SKIN }).stroke({ width: 1.5, color: OUTLINE, alpha: 0.45 });
    this.head.addChild(face);

    this.eyeL = new Graphics();
    this.eyeL.circle(-4, -19, 1.5).fill({ color: OUTLINE });
    this.head.addChild(this.eyeL);
    this.eyeR = new Graphics();
    this.eyeR.circle(4, -19, 1.5).fill({ color: OUTLINE });
    this.head.addChild(this.eyeR);

    this.mouth = new Graphics();
    this.drawMouthNeutral();
    this.head.addChild(this.mouth);

    this.body.addChild(this.head);

    this.accessory = new Container();
    this.accessory.position.set(0, -25);
    this.drawAccessory();
    this.body.addChild(this.accessory);

    this.badgeBg = new Graphics();
    this.badge = new Text({
      text: name,
      style: new TextStyle({
        fontSize: 11,
        fill: 0x0f172a,
        fontWeight: '600',
        fontFamily: 'ui-sans-serif, system-ui',
      }),
    });
    this.badge.anchor.set(0.5, 0);
    this.badge.position.set(0, 40);
    this.drawBadgeBg();
    this.addChild(this.badgeBg);
    this.addChild(this.badge);

    if (emoji && emoji.length && !/^(🧭|🧠|🔌|⚙️|🚒)$/.test(emoji)) {
      const emojiText = new Text({
        text: emoji,
        style: new TextStyle({ fontSize: 14 }),
      });
      emojiText.anchor.set(0.5);
      emojiText.position.set(14, -26);
      this.addChild(emojiText);
    }

    this.animationMap = {
      idle: (dt) => this.animIdle(dt),
      walk: (dt) => this.animWalk(dt),
      talk: (dt) => this.animTalk(dt),
      work: (dt) => this.animWork(dt),
      happy: (dt) => this.animHappy(dt),
      alert: (dt) => this.animAlert(dt),
    };

    this.eventMode = 'static';
    this.cursor = 'pointer';
  }

  // ── API pública (padrão Spine Boy) ─────────────────────────────
  playAnimation(state: AgentState) {
    if (this.state === state) return;
    this.prevState = this.state;
    this.state = state;
    this.mix = 0;
    this.stateTimer = 0;
    if (state === 'happy' || state === 'talk') this.drawMouthSmile();
    else this.drawMouthNeutral();
  }

  isAnimationPlaying(state: AgentState) {
    return this.state === state;
  }

  setDirection(dir: -1 | 1) {
    this.directionalView.scale.x = dir;
  }

  // ── desenhos base ──────────────────────────────────────────────
  private makeLeg(x: number): Graphics {
    const g = new Graphics();
    g.roundRect(-3, 0, 6, 10, 1.5).fill({ color: this.palette.accent });
    g.position.set(x, 14);
    return g;
  }

  private makeArm(x: number): Graphics {
    const g = new Graphics();
    g.roundRect(-2.5, 0, 5, 13, 2).fill({ color: this.palette.primary }).stroke({ width: 1, color: OUTLINE, alpha: 0.35 });
    g.circle(0, 13, 2.5).fill({ color: SKIN });
    g.position.set(x, -6);
    return g;
  }

  private drawMouthNeutral() {
    this.mouth.clear();
    this.mouth.moveTo(-2, -14).lineTo(2, -14).stroke({ width: 1.2, color: OUTLINE });
  }

  private drawMouthSmile() {
    this.mouth.clear();
    this.mouth
      .moveTo(-2.5, -14.5)
      .quadraticCurveTo(0, -13, 2.5, -14.5)
      .stroke({ width: 1.2, color: OUTLINE });
  }

  private drawAccessory() {
    this.accessory.removeChildren();
    const g = new Graphics();
    const p = this.palette;
    switch (this.type) {
      case 'core': {
        g.moveTo(-9, 0).lineTo(-9, -4).lineTo(-5, -1).lineTo(-2, -7).lineTo(2, -1).lineTo(5, -7).lineTo(9, -1).lineTo(9, 0).closePath();
        g.fill({ color: p.gear }).stroke({ width: 1, color: OUTLINE });
        const gem = new Graphics();
        gem.circle(0, -3, 1.5).fill({ color: 0xec4899 });
        this.accessory.addChild(g);
        this.accessory.addChild(gem);
        break;
      }
      case 'specialist': {
        g.circle(-4, 0, 3).stroke({ width: 1.2, color: OUTLINE });
        g.circle(4, 0, 3).stroke({ width: 1.2, color: OUTLINE });
        g.moveTo(-1.5, 0).lineTo(1.5, 0).stroke({ width: 1.2, color: OUTLINE });
        g.y = 6;
        this.accessory.addChild(g);
        const hair = new Graphics();
        hair.roundRect(-10, -6, 20, 5, 3).fill({ color: p.accent });
        this.accessory.addChild(hair);
        break;
      }
      case 'integrator': {
        const stem = new Graphics();
        stem.rect(-1, -8, 2, 8).fill({ color: p.accent });
        stem.circle(0, -10, 3).fill({ color: p.gear }).stroke({ width: 1, color: OUTLINE });
        this.accessory.addChild(stem);
        const cap = new Graphics();
        cap.roundRect(-8, -2, 16, 4, 2).fill({ color: p.primary });
        this.accessory.addChild(cap);
        break;
      }
      case 'motor': {
        const gear = new Graphics();
        const teeth = 8;
        const R = 5;
        const r = 3.5;
        for (let i = 0; i < teeth; i++) {
          const a = (i / teeth) * Math.PI * 2;
          const x = Math.cos(a) * R;
          const y = Math.sin(a) * R - 4;
          gear.rect(x - 1, y - 1, 2, 2).fill({ color: p.gear });
        }
        gear.circle(0, -4, r).fill({ color: p.gear }).stroke({ width: 1, color: OUTLINE });
        gear.circle(0, -4, 1).fill({ color: OUTLINE });
        this.accessory.addChild(gear);
        const antenna = new Graphics();
        antenna.moveTo(6, -4).lineTo(8, -10).stroke({ width: 1, color: OUTLINE });
        antenna.circle(8, -10, 1.5).fill({ color: 0xef4444 });
        this.accessory.addChild(antenna);
        break;
      }
      case 'firefighter': {
        g.moveTo(-10, 0).quadraticCurveTo(-10, -9, 0, -9).quadraticCurveTo(10, -9, 10, 0).closePath();
        g.fill({ color: p.primary }).stroke({ width: 1.2, color: OUTLINE });
        g.roundRect(-10, -2, 20, 2, 1).fill({ color: p.gear });
        const badge = new Graphics();
        badge.circle(0, -5, 1.8).fill({ color: p.gear }).stroke({ width: 0.8, color: OUTLINE });
        this.accessory.addChild(g);
        this.accessory.addChild(badge);
        break;
      }
    }
  }

  private drawBadgeBg() {
    const m = this.badge.width + 10;
    this.badgeBg.clear();
    this.badgeBg
      .roundRect(-m / 2, 38, m, 16, 8)
      .fill({ color: 0xffffff, alpha: 0.92 })
      .stroke({ width: 1, color: this.palette.primary, alpha: 0.35 });
  }

  private drawStatus(kind: 'none' | 'calm' | 'alert') {
    if (kind === this.statusKind) return;
    this.statusKind = kind;
    if (this.statusMark) { this.statusMark.destroy(); this.statusMark = null; }
    if (this.statusBg) { this.statusBg.destroy(); this.statusBg = null; }
    if (kind === 'none') return;
    const isAlert = kind === 'alert';
    const color = isAlert ? 0xef4444 : 0x22c55e;
    const label = isAlert ? '!' : '✓';
    this.statusBg = new Graphics();
    this.statusBg.circle(14, -36, 9).fill({ color }).stroke({ width: 1.5, color: 0xffffff });
    this.statusMark = new Text({
      text: label,
      style: new TextStyle({
        fontSize: isAlert ? 16 : 12,
        fill: 0xffffff,
        fontWeight: '900',
        fontFamily: 'ui-sans-serif, system-ui',
      }),
    });
    this.statusMark.anchor.set(0.5);
    this.statusMark.position.set(14, -36);
    this.addChild(this.statusBg);
    this.addChild(this.statusMark);
  }

  public setErrorTooltip(msg: string | null, hint: string | null) {
    const key = msg == null ? null : `${msg}|${hint ?? ''}`;
    if (key === this.errorBubbleKey) return;
    this.errorBubbleKey = key;
    if (this.errorBubble) {
      this.errorBubble.destroy({ children: true });
      this.errorBubble = null;
    }
    if (!msg) return;

    const bubble = new Container();
    const wrapWidth = 180;
    const trunc = (s: string, n: number) => (s.length > n ? s.slice(0, n - 1) + '…' : s);

    const errText = new Text({
      text: `Erro: ${trunc(msg, 140)}`,
      style: new TextStyle({
        fontSize: 9,
        fill: 0x7f1d1d,
        fontWeight: '600',
        fontFamily: 'ui-sans-serif, system-ui',
        wordWrap: true,
        wordWrapWidth: wrapWidth,
      }),
    });
    const hintText = hint
      ? new Text({
          text: `Correção: ${trunc(hint, 140)}`,
          style: new TextStyle({
            fontSize: 9,
            fill: 0x0f172a,
            fontFamily: 'ui-sans-serif, system-ui',
            wordWrap: true,
            wordWrapWidth: wrapWidth,
          }),
        })
      : null;

    const padX = 6;
    const padY = 5;
    const gap = 2;
    const copyBtnSize = 14;
    const copyGap = 4;
    const contentW = Math.min(Math.max(errText.width, hintText ? hintText.width : 0), wrapWidth);
    const w = contentW + padX * 2 + copyBtnSize + copyGap;
    const h = errText.height + (hintText ? hintText.height + gap : 0) + padY * 2;

    const bg = new Graphics();
    bg.roundRect(-w / 2, 0, w, h, 6)
      .fill({ color: 0xffffff, alpha: 0.96 })
      .stroke({ width: 1.2, color: 0xef4444 });
    bg.moveTo(-4, h)
      .lineTo(0, h + 5)
      .lineTo(4, h)
      .closePath()
      .fill({ color: 0xffffff })
      .stroke({ width: 1.2, color: 0xef4444 });

    errText.position.set(-w / 2 + padX, padY);
    if (hintText) hintText.position.set(-w / 2 + padX, padY + errText.height + gap);

    const copyBtn = new Container();
    const copyBg = new Graphics();
    copyBg.roundRect(0, 0, copyBtnSize, copyBtnSize, 3)
      .fill({ color: 0xf1f5f9 })
      .stroke({ width: 0.8, color: 0xef4444, alpha: 0.5 });
    const copyIcon = new Text({
      text: '📋',
      style: new TextStyle({ fontSize: 9, fontFamily: 'ui-sans-serif, system-ui' }),
    });
    copyIcon.anchor.set(0.5);
    copyIcon.position.set(copyBtnSize / 2, copyBtnSize / 2);
    copyBtn.addChild(copyBg);
    copyBtn.addChild(copyIcon);
    copyBtn.position.set(w / 2 - copyBtnSize - padX / 2, padY);
    copyBtn.eventMode = 'static';
    copyBtn.cursor = 'pointer';
    copyBtn.hitArea = { contains: (x: number, y: number) =>
      x >= 0 && x <= copyBtnSize && y >= 0 && y <= copyBtnSize } as any;
    const payload = hint ? `Erro: ${msg}\nCorreção: ${hint}` : `Erro: ${msg}`;
    const copyHandler = (e: any) => {
      if (e?.stopPropagation) e.stopPropagation();
      try {
        navigator.clipboard?.writeText(payload);
        copyIcon.text = '✓';
        setTimeout(() => { copyIcon.text = '📋'; }, 900);
      } catch {}
    };
    copyBtn.on('pointerdown', copyHandler);
    copyBtn.on('pointertap', copyHandler);

    bubble.addChild(bg);
    bubble.addChild(errText);
    if (hintText) bubble.addChild(hintText);
    bubble.addChild(copyBtn);

    bubble.position.set(0, -50 - h);
    this.errorBubble = bubble;
    this.addChild(bubble);
  }

  public updateName(name: string) {
    this.name = name;
    this.badge.text = name;
    this.drawBadgeBg();
  }

  public updateType(type: AgentType, profession?: string) {
    if (type === this.type && profession === this.profession) return;
    this.type = type;
    this.profession = profession;
    this.palette = paletteFor(type, profession);
    this.workingKind = {
      core: 'compass',
      specialist: 'sparkle',
      integrator: 'plug',
      motor: 'gear',
      firefighter: 'fire',
    }[type] as any;
    this.body.removeChildren();
    this.legL = this.makeLeg(-6);
    this.legR = this.makeLeg(6);
    this.body.addChild(this.legL);
    this.body.addChild(this.legR);
    const torso = new Graphics();
    torso
      .roundRect(-11, -8, 22, 22, 6)
      .fill({ color: this.palette.primary })
      .stroke({ width: 1.5, color: OUTLINE, alpha: 0.35 });
    torso.rect(-11, 2, 22, 3).fill({ color: this.palette.accent, alpha: 0.7 });
    this.body.addChild(torso);
    this.armL = this.makeArm(-11);
    this.armR = this.makeArm(11);
    this.body.addChild(this.armL);
    this.body.addChild(this.armR);
    this.body.addChild(this.head);
    this.accessory.position.set(0, -25);
    this.drawAccessory();
    this.body.addChild(this.accessory);
    this.drawBadgeBg();
  }

  // Wrappers que resolvem estados com base em prioridade (igual Spine Boy)
  public setSelected(sel: boolean) {
    if (sel) {
      this.scale.set(1.1);
      // talk só assume se nada de maior prioridade estiver ativo
      if (STATE_PRIORITY[this.state] <= STATE_PRIORITY.talk) this.playAnimation('talk');
    } else {
      this.scale.set(1);
      if (this.state === 'talk') this.playAnimation('idle');
    }
  }

  public setWorking(on: boolean) {
    if (on) {
      if (STATE_PRIORITY[this.state] <= STATE_PRIORITY.work) this.playAnimation('work');
    } else if (this.state === 'work') {
      this.playAnimation('idle');
    }
  }

  public setStatus(kind: 'none' | 'calm' | 'alert') {
    this.drawStatus(kind);
    if (kind === 'alert') this.playAnimation('alert');
    else if (this.state === 'alert') this.playAnimation('idle');
  }

  public greet() {
    if (this.state === 'alert' || this.state === 'work') return;
    this.playAnimation('happy');
  }

  // ── animações por estado ───────────────────────────────────────
  private animIdle(dt: number) {
    this.body.y = lerp(this.body.y, Math.sin(this.t * 0.06) * 0.9, 0.12);
    this.armL.rotation = lerp(this.armL.rotation, 0, 0.2);
    this.armR.rotation = lerp(this.armR.rotation, 0, 0.2);
    if (this.type === 'motor') this.accessory.rotation *= 0.9;
  }

  private animTalk(dt: number) {
    // respiração mais animada + leve aceno
    this.body.y = lerp(this.body.y, Math.sin(this.t * 0.1) * 1.2, 0.15);
    const wave = Math.sin(this.t * 0.22) * 0.35;
    this.armR.rotation = lerp(this.armR.rotation, wave, 0.2);
    this.armL.rotation = lerp(this.armL.rotation, 0, 0.2);
  }

  private animWalk(dt: number) {
    const s = Math.sin(this.t * 0.28);
    this.body.y = lerp(this.body.y, Math.abs(s) * -1.5, 0.2);
    this.legL.rotation = s * 0.4;
    this.legR.rotation = -s * 0.4;
    this.armL.rotation = -s * 0.3;
    this.armR.rotation = s * 0.3;
  }

  private animWork(dt: number) {
    this.body.y = lerp(this.body.y, Math.sin(this.t * 0.22) * 2.2, 0.2);
    const sw = Math.sin(this.t * 0.3) * 0.7;
    this.armL.rotation = sw;
    this.armR.rotation = -sw;
    if (this.type === 'motor') this.accessory.rotation = this.t * 0.05;
    if (Math.random() < 0.22) this.emitParticle();
  }

  private animHappy(dt: number) {
    this.stateTimer += dt / 60;
    // aceno mais rápido com braço direito
    const wave = Math.sin(this.t * 0.5) * 0.9 - 0.3;
    this.armR.rotation = lerp(this.armR.rotation, wave, 0.3);
    this.armL.rotation = lerp(this.armL.rotation, 0, 0.2);
    this.body.y = lerp(this.body.y, Math.sin(this.t * 0.25) * 1.6, 0.2);
    // auto-retorno após ~1.6s
    if (this.stateTimer > 1.6) this.playAnimation('idle');
  }

  private animAlert(dt: number) {
    this.body.y = lerp(this.body.y, Math.sin(this.t * 0.6) * 1.8, 0.3);
    const shake = Math.sin(this.t * 0.9) * 0.15;
    this.body.x = shake * 4;
    this.armL.rotation = shake * 2;
    this.armR.rotation = -shake * 2;
    if (Math.random() < 0.3) this.emitParticle();
    if (this.statusMark && this.statusKind === 'alert') {
      const s = 1 + Math.sin(this.t * 0.4) * 0.12;
      this.statusMark.scale.set(s);
    }
  }

  public tick(t: Ticker) {
    const dt = t.deltaTime;
    this.t += dt;
    this.blinkT += dt;

    // cross-fade (defaultMix): blend suave entre estado anterior e atual
    if (this.mix < 1) {
      this.mix = Math.min(1, this.mix + dt / (60 * this.defaultMix));
      this.animationMap[this.prevState](dt);
    }
    this.animationMap[this.state](dt);

    this.shadow.scale.x = 1 - Math.abs(this.body.y) * 0.02;

    if (this.blinkClose > 0) {
      this.blinkClose -= dt;
      this.eyeL.scale.y = 0.15;
      this.eyeR.scale.y = 0.15;
    } else {
      this.eyeL.scale.y = 1;
      this.eyeR.scale.y = 1;
      if (this.blinkT > 120 + Math.random() * 100) {
        this.blinkT = 0;
        this.blinkClose = 8;
      }
    }

    // partículas
    for (let i = this.particles.length - 1; i >= 0; i--) {
      const p = this.particles[i];
      p.life -= dt;
      p.g.x += p.vx * dt;
      p.g.y += p.vy * dt;
      p.vy += 0.04 * dt;
      p.g.alpha = Math.max(0, p.life / p.max);
      if (p.life <= 0) {
        this.particleLayer.removeChild(p.g);
        p.g.destroy();
        this.particles.splice(i, 1);
      }
    }
  }

  private emitParticle() {
    const g = new Graphics();
    const kind = this.workingKind;
    let maxLife = 50;
    let vx = (Math.random() - 0.5) * 0.8;
    let vy = -1.2 - Math.random() * 0.5;
    let startX = (Math.random() - 0.5) * 8;
    let startY = 0;

    switch (kind) {
      case 'fire':
        g.circle(0, 0, 2 + Math.random() * 1.5).fill({
          color: Math.random() > 0.5 ? 0xfb923c : 0xef4444,
          alpha: 0.9,
        });
        startY = -28;
        vy = -1.8 - Math.random() * 0.5;
        maxLife = 40;
        break;
      case 'gear':
        g.rect(-2, -2, 4, 4)
          .fill({ color: 0x94a3b8, alpha: 0.85 })
          .stroke({ width: 0.6, color: OUTLINE });
        startY = -8;
        vy = -0.5 - Math.random();
        maxLife = 55;
        break;
      case 'plug':
        g.moveTo(0, -3).lineTo(2, 0).lineTo(0, 3).lineTo(-2, 0).closePath();
        g.fill({ color: 0x22d3ee, alpha: 0.9 });
        startY = -30;
        vy = -2 - Math.random();
        maxLife = 35;
        break;
      case 'sparkle':
        g.star(0, 0, 4, 2.5, 1.1).fill({ color: 0xfde047, alpha: 0.95 });
        startY = -20;
        vy = -1.2 - Math.random() * 0.6;
        maxLife = 50;
        break;
      case 'compass':
        g.circle(0, 0, 1.8).fill({ color: 0xfbbf24, alpha: 0.9 });
        startY = -25;
        vy = -1.5;
        vx = (Math.random() - 0.5) * 1.4;
        maxLife = 45;
        break;
      default:
        g.circle(0, 0, 2).fill({ color: 0xffffff, alpha: 0.8 });
        maxLife = 40;
    }
    g.x = startX;
    g.y = startY;
    this.particleLayer.addChild(g);
    this.particles.push({ g, vx, vy, life: maxLife, max: maxLife });
  }
}

function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t;
}
