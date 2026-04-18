import { Container, Graphics, Text, TextStyle, Ticker } from 'pixi.js';
import type { HumanGender, HumanSkin } from '@/lib/humans';

const OUTLINE = 0x0f172a;
const SHADOW = 0x0f172a;

export class HumanSprite extends Container {
  public humanName: string;
  public humanId: string;
  private skin: HumanSkin;
  private gender: HumanGender;
  private isMe: boolean;
  private isOnline: boolean;
  private shadow: Graphics;
  private body: Container;
  private head: Container;
  private eyeL: Graphics;
  private eyeR: Graphics;
  private ring: Graphics | null = null;
  private offlineBadge: Graphics | null = null;
  private badge: Text;
  private badgeBg: Graphics;
  private t = 0;
  private blinkT = 0;
  private blinkClose = 0;
  private falling = false;
  private landAnim = 0; // frames restantes da animação de pouso (squash & stretch)

  constructor(
    humanId: string,
    humanName: string,
    skin: HumanSkin,
    opts: { isMe?: boolean; online?: boolean; gender?: HumanGender | null } = {},
  ) {
    super();
    this.humanId = humanId;
    this.humanName = humanName;
    this.skin = skin;
    this.gender = (opts.gender as HumanGender) || 'other';
    this.isMe = !!opts.isMe;
    this.isOnline = opts.online !== false;

    this.shadow = new Graphics();
    this.shadow.ellipse(0, 30, 20, 4.5).fill({ color: SHADOW, alpha: 0.18 });
    this.addChild(this.shadow);

    if (this.isMe) {
      this.ring = new Graphics();
      this.addChild(this.ring);
      this.drawRing();
    }

    this.body = new Container();
    this.addChild(this.body);

    const isDress = skin.outfit === 'dress';

    if (isDress) {
      // pernas expostas (cor da pele)
      const legL = new Graphics();
      legL.roundRect(-2.5, 0, 5, 12, 1.5).fill({ color: skin.skinTone });
      legL.position.set(-4, 14);
      this.body.addChild(legL);
      const legR = new Graphics();
      legR.roundRect(-2.5, 0, 5, 12, 1.5).fill({ color: skin.skinTone });
      legR.position.set(4, 14);
      this.body.addChild(legR);
    } else {
      // pernas / calça
      const legL = new Graphics();
      legL.roundRect(-3, 0, 6, 12, 1.5).fill({ color: skin.pants });
      legL.position.set(-4, 14);
      this.body.addChild(legL);
      const legR = new Graphics();
      legR.roundRect(-3, 0, 6, 12, 1.5).fill({ color: skin.pants });
      legR.position.set(4, 14);
      this.body.addChild(legR);
    }

    // sapatos
    const shoes = new Graphics();
    shoes.roundRect(-8, 26, 8, 3, 1.5).fill({ color: 0x0f172a });
    shoes.roundRect(0, 26, 8, 3, 1.5).fill({ color: 0x0f172a });
    this.body.addChild(shoes);

    // torso (camisa/vestido)
    const torso = new Graphics();
    torso
      .roundRect(-10, -7, 20, 20, 5)
      .fill({ color: skin.shirt })
      .stroke({ width: 1.2, color: OUTLINE, alpha: 0.35 });
    torso.rect(-10, 11, 20, 3).fill({ color: 0x00000022 });
    this.body.addChild(torso);

    if (isDress) {
      // saia trapezoidal cobrindo topo das pernas
      const skirt = new Graphics();
      skirt
        .moveTo(-10, 11)
        .lineTo(10, 11)
        .lineTo(15, 24)
        .lineTo(-15, 24)
        .closePath()
        .fill({ color: skin.shirt })
        .stroke({ width: 1.2, color: OUTLINE, alpha: 0.35 });
      this.body.addChild(skirt);
    }

    // braços
    const armL = new Graphics();
    armL.roundRect(-2.5, 0, 5, 12, 2)
      .fill({ color: skin.shirt })
      .stroke({ width: 1, color: OUTLINE, alpha: 0.3 });
    armL.circle(0, 12, 2.8).fill({ color: skin.skinTone });
    armL.position.set(-10, -5);
    this.body.addChild(armL);
    const armR = new Graphics();
    armR.roundRect(-2.5, 0, 5, 12, 2)
      .fill({ color: skin.shirt })
      .stroke({ width: 1, color: OUTLINE, alpha: 0.3 });
    armR.circle(0, 12, 2.8).fill({ color: skin.skinTone });
    armR.position.set(10, -5);
    this.body.addChild(armR);

    // cabeça
    this.head = new Container();

    const isFemale = this.gender === 'female';

    // cabelo de trás (feminino) — desenhado ANTES do rosto, cai até os ombros
    if (isFemale) {
      const backHair = new Graphics();
      backHair
        .moveTo(-13, -15)
        .quadraticCurveTo(-18, -22, -11, -28)
        .quadraticCurveTo(0, -32, 11, -28)
        .quadraticCurveTo(18, -22, 13, -15)
        .lineTo(13, 12)
        .quadraticCurveTo(10, 15, 6, 12)
        .lineTo(6, -8)
        .lineTo(-6, -8)
        .lineTo(-6, 12)
        .quadraticCurveTo(-10, 15, -13, 12)
        .closePath()
        .fill({ color: skin.hair })
        .stroke({ width: 1, color: OUTLINE, alpha: 0.25 });
      this.head.addChild(backHair);
    }

    const face = new Graphics();
    face.circle(0, -18, 10).fill({ color: skin.skinTone }).stroke({ width: 1.2, color: OUTLINE, alpha: 0.4 });
    this.head.addChild(face);

    // cabelo (topo + franja)
    const hair = new Graphics();
    if (isFemale) {
      // topo mais cheio + franja lateral
      hair.ellipse(0, -24, 12, 8).fill({ color: skin.hair });
      hair
        .moveTo(-11, -19)
        .quadraticCurveTo(-6, -28, 4, -22)
        .quadraticCurveTo(8, -20, 11, -19)
        .lineTo(11, -16)
        .quadraticCurveTo(0, -22, -11, -16)
        .closePath()
        .fill({ color: skin.hair });
    } else {
      hair.ellipse(0, -24, 11, 7).fill({ color: skin.hair });
      hair
        .moveTo(-10, -20)
        .quadraticCurveTo(0, -28, 10, -20)
        .lineTo(10, -17)
        .quadraticCurveTo(0, -22, -10, -17)
        .closePath()
        .fill({ color: skin.hair });
    }
    this.head.addChild(hair);

    this.eyeL = new Graphics();
    this.eyeL.circle(-3.5, -18, 1.3).fill({ color: OUTLINE });
    this.head.addChild(this.eyeL);
    this.eyeR = new Graphics();
    this.eyeR.circle(3.5, -18, 1.3).fill({ color: OUTLINE });
    this.head.addChild(this.eyeR);

    const mouth = new Graphics();
    mouth
      .moveTo(-2.5, -14)
      .quadraticCurveTo(0, -12.5, 2.5, -14)
      .stroke({ width: 1.1, color: OUTLINE });
    this.head.addChild(mouth);

    this.body.addChild(this.head);

    // indicador offline (fantasma esmaecido)
    if (!this.isOnline) this.alpha = 0.55;

    // badge de nome
    this.badgeBg = new Graphics();
    this.badge = new Text({
      text: humanName + (this.isMe ? ' (você)' : ''),
      style: new TextStyle({
        fontSize: 11,
        fill: 0x0f172a,
        fontWeight: '700',
        fontFamily: 'ui-sans-serif, system-ui',
      }),
    });
    this.badge.anchor.set(0.5, 0);
    this.badge.position.set(0, 40);
    this.drawBadgeBg();
    this.addChild(this.badgeBg);
    this.addChild(this.badge);

    if (!this.isOnline) this.drawOfflineBadge();

    this.eventMode = 'static';
    this.cursor = 'pointer';
  }

  public tick(t: Ticker) {
    const dt = t.deltaTime;
    this.t += dt;
    this.blinkT += dt;

    // respiração leve (só quando parado no chão)
    if (!this.falling && this.landAnim <= 0) {
      this.body.y = Math.sin(this.t * 0.06) * 0.8;
      this.body.scale.set(1, 1);
      this.body.rotation = 0;
    } else if (this.falling) {
      // queda: corpo levemente oscilando como se estivesse solto no ar
      this.body.y = 0;
      this.body.scale.set(1, 1);
      this.body.rotation = Math.sin(this.t * 0.18) * 0.12;
    } else if (this.landAnim > 0) {
      // pouso: squash & stretch (achata, depois estica, depois volta)
      this.landAnim -= dt;
      const p = 1 - this.landAnim / 18; // 0→1
      // squash nos primeiros 40%, stretch leve até 80%, normal depois
      let sx = 1;
      let sy = 1;
      if (p < 0.4) {
        const k = p / 0.4;
        sy = 1 - 0.25 * (1 - k);
        sx = 1 + 0.18 * (1 - k);
      } else if (p < 0.8) {
        const k = (p - 0.4) / 0.4;
        sy = 0.75 + 0.3 * k;
        sx = 1.18 - 0.22 * k;
      } else {
        const k = (p - 0.8) / 0.2;
        sy = 1.05 - 0.05 * k;
        sx = 0.96 + 0.04 * k;
      }
      this.body.scale.set(sx, sy);
      this.body.y = 0;
      this.body.rotation = 0;
    }

    if (this.blinkClose > 0) {
      this.blinkClose -= dt;
      this.eyeL.scale.y = 0.15;
      this.eyeR.scale.y = 0.15;
    } else {
      this.eyeL.scale.y = 1;
      this.eyeR.scale.y = 1;
      if (this.blinkT > 150 + Math.random() * 120) {
        this.blinkT = 0;
        this.blinkClose = 8;
      }
    }

    if (this.ring && this.isMe) {
      const s = 1 + Math.sin(this.t * 0.1) * 0.05;
      this.ring.scale.set(s);
    }
  }

  public setFalling(on: boolean) {
    this.falling = on;
    if (!on) this.body.rotation = 0;
  }

  public landBounce() {
    this.falling = false;
    this.landAnim = 18;
  }

  private drawBadgeBg() {
    const m = this.badge.width + 10;
    this.badgeBg.clear();
    const fill = this.isMe ? 0xdbeafe : 0xffffff;
    const stroke = this.isMe ? 0x2563eb : 0x94a3b8;
    this.badgeBg
      .roundRect(-m / 2, 38, m, 16, 8)
      .fill({ color: fill, alpha: 0.95 })
      .stroke({ width: 1, color: stroke, alpha: 0.7 });
  }

  private drawRing() {
    if (!this.ring) return;
    this.ring.clear();
    this.ring
      .circle(0, 0, 34)
      .stroke({ width: 2, color: 0x2563eb, alpha: 0.55 });
  }

  private drawOfflineBadge() {
    if (this.offlineBadge) return;
    const g = new Graphics();
    g.circle(14, -28, 6).fill({ color: 0x94a3b8 }).stroke({ width: 1.2, color: 0xffffff });
    this.addChild(g);
    const txt = new Text({
      text: 'zZ',
      style: new TextStyle({
        fontSize: 8,
        fill: 0xffffff,
        fontWeight: '800',
        fontFamily: 'ui-sans-serif, system-ui',
      }),
    });
    txt.anchor.set(0.5);
    txt.position.set(14, -28);
    this.addChild(txt);
    this.offlineBadge = g;
  }
}
