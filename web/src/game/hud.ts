// The interface as DOM over the canvas: her health and mana, the count of
// knights undone, the chain of strikes.

import { SNAP, TUNE } from "../sim/abi.gen";

export class Hud {
  private hp: HTMLElement;
  private mana: HTMLElement;
  private kos: HTMLElement;
  private chain: HTMLElement;
  private note: HTMLElement;
  private noteUntil = 0;

  constructor(root: HTMLElement) {
    root.innerHTML = `
      <div class="gauge"><div class="label">MAGE</div><div class="bar"><div class="fill" id="hp"></div></div><div class="bar thin"><div class="fill mana" id="mana"></div></div></div>
      <div class="score"><span id="kos">0</span><small id="goal"></small></div>
      <div class="chain" id="chain"></div>
      <div class="note" id="note"></div>
      <div class="help">WASD move &nbsp; J strike &nbsp; K spell &nbsp; SPACE evade &nbsp; L unseal &nbsp; Q guard &nbsp; SHIFT hover &nbsp; arrows camera</div>`;
    this.hp = root.querySelector("#hp")!;
    this.mana = root.querySelector("#mana")!;
    this.kos = root.querySelector("#kos")!;
    this.chain = root.querySelector("#chain")!;
    this.note = root.querySelector("#note")!;
  }

  say(text: string, seconds = 1.6) {
    this.note.textContent = text;
    this.note.classList.add("on");
    this.noteUntil = performance.now() + seconds * 1000;
  }

  update(s: Float32Array) {
    const hp = s[SNAP.HP] / TUNE.HP_MAX;
    this.hp.style.width = `${hp * 100}%`;
    this.hp.classList.toggle("low", hp < 0.25);
    const mana = s[SNAP.MANA] / TUNE.MANA_MAX;
    this.mana.style.width = `${mana * 100}%`;
    this.mana.classList.toggle("full", mana >= 1);
    this.kos.textContent = `${s[SNAP.KOS]}`;
    (this.kos.nextElementSibling as HTMLElement).textContent = ` / ${s[SNAP.GOAL]}`;
    const chain = s[SNAP.CHAIN];
    this.chain.textContent = chain >= 3 ? `${chain} HITS` : "";
    if (performance.now() > this.noteUntil) this.note.classList.remove("on");
  }
}
