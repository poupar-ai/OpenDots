import { Container, Graphics, Text, TextStyle, Ticker } from 'pixi.js';
import type { HumanSkin } from '@/lib/humans';

const OUTLINE = 0x0f172a;
const SHADOW = 0x0f172a;

export class HumanSprite extends Container {
  public humanName: string;
  public humanId: string;
  private skin: HumanSkin;
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

  constructor(
    humanId: string,
    humanName: string,
    skin: HumanSkin,
    opts: { isMe?: boolean; online?: boolean } = {},
  ) {
    super();
    this.humanId = humanId;
    this.humanName = humanName;
    this.skin = skin;
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

    // pernas / calça
    const legL = new Graphics();
    legL.roundRect(-3, 0, 6, 12, 1.5).fill({ color: skin.pants });
    legL.position.set(-4, 14);
    this.body.addChild(legL);
    const legR = new Graphics();
    legR.roundRect(-3, 0, 6, 12, 1.5).fill({ color: skin.pants });
    legR.position.set(4, 14);
    this.body.addChild(legR);

    // sapatos
    const shoes = new Graphics();
    shoes.roundRect(-8, 26, 8, 3, 1.5).fill({ color: 0x0f172a });
    shoes.roundRect(0, 26, 8, 3, 1.5).fill({ color: 0x0f172a });
    this.body.addChild(shoes);

    // torso (camisa)
    const torso = new Graphics();
    torso
      .roundRect(-10, -7, 20, 20, 5)
      .fill({ color: skin.shirt })
      .stroke({ width: 1.2, color: OUTLINE, alpha: 0.35 });
    torso.rect(-10, 11, 20, 3).fill({ color: 0x00000022 });
    this.body.addChild(torso);

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
    const face = new Graphics();
    face.circle(0, -18, 10).fill({ color: skin.skinTone }).stroke({ width: 1.2, color: OUTLINE, alpha: 0.4 });
    this.head.addChild(face);

    // cabelo (topo + franja)
    const hair = new Graphics();
    hair.ellipse(0, -24, 11, 7).fill({ color: skin.hair });
    hair
      .moveTo(-10, -20)
      .quadraticCurveTo(0, -28, 10, -20)
      .lineTo(10, -17)
      .quadraticCurveTo(0, -22, -10, -17)
      .closePath()
      .fill({ color: skin.hair });
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

    // respiração leve
    this.body.y = Math.sin(this.t * 0.06) * 0.8;

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
