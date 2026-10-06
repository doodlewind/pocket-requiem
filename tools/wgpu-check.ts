// The page in Chrome with WebGPU, driven as a person drives it (bun tools/wgpu.ts check).
//
// Each device from its address: the title card, the pack read, the fight; no error in the console; the
// frames a second over a few seconds and what a frame costs without the display; the pad taken from the
// autopilot by a key and by a key of the shell, the stick, START. Then on the PS Vita: the mage close up,
// the army closed round her while she stands still, another device picked in the fight and the 3DS's
// lower screen. Then a phone's window, and a browser without WebGPU.

import { writeFileSync } from "node:fs";
import { join } from "node:path";

interface Options {
  origin: string;
  directory: string;
  seconds: number;
  headed: boolean;
  deployed: boolean;
  opens: { query: string; mode: string }[];
}

const DEVICES = ["vita", "psp", "3ds"] as const;

export async function check({ origin, directory, seconds, headed, deployed, opens }: Options) {
  const { chromium } = await import("playwright-core");
  // WebGPU needs the real GPU: headless Chrome is given Metal through ANGLE; --headed opens a window instead.
  const browser = await chromium.launch({ channel: "chrome", headless: !headed, args: ["--use-angle=metal", "--enable-gpu", "--ignore-gpu-blocklist", "--enable-unsafe-webgpu", "--no-proxy-server", "--autoplay-policy=no-user-gesture-required"] });
  const report: Record<string, any> = { chrome: browser.version() };
  const expect = (what: string, ok: boolean) => {
    if (!ok) throw new Error(`the page: ${what}`);
  };
  type Context = Awaited<ReturnType<typeof browser.newContext>>;
  const visit = async (address: string, context?: Context, viewport = { width: 1440, height: 900 }) => {
    const page = await (context ?? browser).newPage(context ? undefined : { viewport, deviceScaleFactor: 2 });
    const problems: string[] = [];
    // (the deployable directory's host answers no /app.json: the browser writes that one reply to the
    // console, and the player goes on with the page's own words)
    page.on("console", (message) => message.type() === "error" && !message.location().url.endsWith("/app.json") && problems.push(message.text()));
    page.on("pageerror", (error) => problems.push(String(error)));
    await page.goto(`${origin}/${address}`);
    const status = async () => JSON.parse((await page.evaluate("pocketRequiem.requiem.status()")) as string);
    return {
      page,
      problems,
      status,
      /** The card has left and the game is on the screen. */
      async up() {
        await page.waitForFunction("window.pocketRequiem && (pocketRequiem.firstGame || pocketRequiem.failure)", undefined, { timeout: 180_000 });
        const failure = (await page.evaluate("pocketRequiem.failure")) as string;
        if (failure) throw new Error(`${address}: ${failure}`);
      },
      async until(tick: number) {
        await page.waitForFunction(`JSON.parse(pocketRequiem.requiem.status()).player.tick >= ${tick}`, undefined, { timeout: 120_000 });
      },
      async key(code: string, hold = 120) {
        await page.keyboard.down(code);
        await page.waitForTimeout(hold);
        await page.keyboard.up(code);
        await page.waitForTimeout(150);
      },
      /** The game's own canvas, one pixel of the game to a pixel. */
      async frame(name: string) {
        const shot = (await page.evaluate("pocketRequiem.capture()")) as { upper: string; lower: string | null };
        writeFileSync(join(directory, `${name}.png`), Buffer.from(shot.upper.split(",")[1]!, "base64"));
        if (shot.lower) writeFileSync(join(directory, `${name}-lower.png`), Buffer.from(shot.lower.split(",")[1]!, "base64"));
      },
    };
  };

  try {
    for (const device of DEVICES) {
      const v = await visit(`?device=${device}`);
      await v.up();
      const times = (await v.page.evaluate("({ firstFrame: pocketRequiem.firstFrame, firstGame: pocketRequiem.firstGame })")) as { firstFrame: number; firstGame: number };
      // The fight, played by the autopilot.
      await v.until(360);
      const from = (await v.page.evaluate("pocketRequiem.frames")) as number;
      await v.page.waitForTimeout(seconds * 1000);
      const fps = (((await v.page.evaluate("pocketRequiem.frames")) as number) - from) / seconds;
      const fight = await v.status();
      expect(`${device}: the fight does not run`, fight.stage === "running" && fight.crowd.shown > 0 && fight.settings.auto);
      expect(`${device}: ${fps.toFixed(1)} frames a second, not ${fight.hz}`, Math.abs(fps - fight.hz) < fight.hz * 0.1);
      await v.page.screenshot({ path: join(directory, `${device}-page.png`) });
      await v.frame(`${device}-fight`);
      const cost = (await v.page.evaluate("pocketRequiem.burst(120)")) as number;
      // A key takes the pad from the autopilot (the face button at the left: a strike).
      await v.key("KeyC");
      expect(`${device}: a key did not take the pad`, (await v.status()).settings.auto === false);
      // The stick moves her.
      const before = (await v.status()).player.pos as number[];
      await v.key("KeyW", 900);
      const after = (await v.status()).player.pos as number[];
      expect(`${device}: the stick did not move her`, Math.hypot(after[0]! - before[0]!, after[2]! - before[2]!) > 1);
      // START hands the pad back, and a key of the shell under the pointer takes it again.
      await v.key("Space");
      expect(`${device}: START did not start the autopilot`, (await v.status()).settings.auto === true);
      const key = v.page.locator('[data-pocket-control="square"]').first();
      const box = await key.boundingBox();
      expect(`${device}: the shell has no key at the left of its diamond`, !!box);
      await v.page.mouse.move(box!.x + box!.width / 2, box!.y + box!.height / 2);
      await v.page.mouse.down();
      await v.page.waitForTimeout(150);
      await v.page.mouse.up();
      await v.page.waitForTimeout(150);
      expect(`${device}: the shell's key did not take the pad`, (await v.status()).settings.auto === false);
      // The synthesizer answers with sound once a key has been down: a twentieth of a second of it is not silence.
      const loud = (await v.page.evaluate("(() => { let most = 0; for (const s of pocketRequiem.requiem.sound(2400, 48000)) most = Math.max(most, Math.abs(s)); return most; })()")) as number;
      expect(`${device}: the game makes no sound`, loud > 0.001 && loud <= 1);
      expect(`${device}: the console has ${v.problems.join(" | ")}`, v.problems.length === 0);
      report[device] = { firstFrameMs: Math.round(times.firstFrame), firstGameMs: Math.round(times.firstGame), fps: +fps.toFixed(2), frameCostMs: +cost.toFixed(2), adapter: fight.adapter, tris: fight.tris, draws: fight.draws, knights: fight.crowd.shown, loadMs: fight.loadMs };
      await v.page.close();
    }

    // The PS Vita's screen: the mage close up, then the army closed round her while she stands still.
    {
      const v = await visit("?device=vita");
      await v.up();
      // (she stands where the fight starts, and the eye is held a few steps in front of her)
      await v.page.evaluate('pocketRequiem.requiem.control("reset=1 auto=0 hud=0 view=1.5,-0.25,517.6,0,-0.45,520,38")');
      await v.page.waitForTimeout(1200);
      await v.frame("vita-mage-close");
      await v.page.evaluate('pocketRequiem.requiem.control("reset=1 auto=1 hud=1 view=off")');
      // (the autopilot kills too fast to show a press of knights: she stands still from 14 s on)
      await v.until(840);
      await v.page.evaluate('pocketRequiem.requiem.control("auto=0")');
      await v.page.waitForTimeout(30_000);
      const siege = await v.status();
      const cost = (await v.page.evaluate("pocketRequiem.burst(120)")) as number;
      await v.frame("vita-siege");
      report.siege = { free: siege.crowd.free, knights: siege.crowd.shown, tris: siege.tris, crowdTris: siege.crowd.tris, pulled: siege.crowd.pulled, byLod: siege.crowd.byLod, draws: siege.draws, frameCostMs: +cost.toFixed(2), cpuMs: siege.cpuMs, hp: siege.player.hp };
      expect("the army did not close round her", siege.crowd.free > 100);
      // Another device picked in the fight, from the bar: the screen changes, the fight goes on.
      const tick = siege.player.tick as number;
      await v.page.getByRole("button", { name: "PSP", exact: true }).click();
      await v.page.waitForTimeout(600);
      const psp = await v.status();
      expect("the PSP was not shown", psp.shape === "psp" && psp.size[0] === 480 && psp.player.tick > tick && psp.settings.post === false);
      await v.page.getByRole("button", { name: "Nintendo 3DS", exact: true }).click();
      await v.page.waitForTimeout(1200);
      const n3ds = await v.status();
      expect("the 3DS was not shown", n3ds.shape === "3ds" && n3ds.size[0] === 400);
      const lower = (await v.page.evaluate(`(() => { const c = document.querySelector('[data-pocket-screen="lower"]'); const d = c.getContext("2d").getImageData(0, 0, c.width, c.height).data; let lit = 0; for (let i = 0; i < d.length; i += 4) lit += d[i] + d[i + 1] + d[i + 2] > 24; return { hidden: c.hidden, width: c.width, height: c.height, lit }; })()`)) as { hidden: boolean; width: number; height: number; lit: number };
      expect("the 3DS's lower screen shows no map", !lower.hidden && lower.width === 320 && lower.lit > 20_000);
      await v.page.screenshot({ path: join(directory, "3ds-picked-page.png") });
      await v.frame("3ds-picked");
      await v.page.getByRole("button", { name: "PS Vita", exact: true }).click();
      await v.page.waitForTimeout(600);
      expect("the PS Vita was not shown again", (await v.status()).shape === "vita" && (await v.status()).settings.post === true);
      expect(`the console has ${v.problems.join(" | ")}`, v.problems.length === 0);
      report.picked = { psp: psp.size, n3ds: n3ds.size, lower };
      await v.page.close();
    }

    // A phone's window, with a finger for a pointer: the shell whole, its keys under a thumb.
    {
      const context = await browser.newContext({ viewport: { width: 390, height: 844 }, deviceScaleFactor: 3, hasTouch: true, isMobile: true });
      const v = await visit("", context);
      await v.up();
      await v.until(200);
      const shell = (await v.page.locator("[data-pocket-shell]").boundingBox())!;
      expect("the shell does not fit a phone's window", shell.x >= -1 && shell.x + shell.width <= 391);
      const key = (await v.page.locator('[data-pocket-control="square"]').first().boundingBox())!;
      // (a thumb that rests on the key for a moment, as one does: a tap of no length ends inside one frame)
      const touch = await context.newCDPSession(v.page);
      const point = { x: Math.round(key.x + key.width / 2), y: Math.round(key.y + key.height / 2) };
      await touch.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [point] });
      await v.page.waitForTimeout(250);
      await touch.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
      await v.page.waitForTimeout(300);
      expect("a thumb on the shell's key did not take the pad", (await v.status()).settings.auto === false);
      await v.page.screenshot({ path: join(directory, "phone-page.png") });
      expect(`a phone's console has ${v.problems.join(" | ")}`, v.problems.length === 0);
      report.phone = { shell: [Math.round(shell.width), Math.round(shell.height)], shape: (await v.status()).shape };
      await context.close();
    }

    // A browser without WebGPU is told so.
    {
      const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
      await context.addInitScript("delete Navigator.prototype.gpu");
      const page = await context.newPage();
      await page.goto(`${origin}/`);
      await page.waitForFunction('document.querySelector("[data-pocket-say]")?.textContent.includes("WebGPU")', undefined, { timeout: 30_000 });
      report.withoutWebGPU = await page.locator("[data-pocket-say]").textContent();
      await context.close();
    }
    // The first picture over a line of 16 Mbit/s with 40 ms of latency (Chrome's own throttle), nothing cached:
    // the pack is read whole before the fight starts.
    {
      const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
      const v = await visit("", context);
      const line = await context.newCDPSession(v.page);
      await line.send("Network.enable");
      await line.send("Network.setCacheDisabled", { cacheDisabled: true });
      await line.send("Network.emulateNetworkConditions", { offline: false, latency: 40, downloadThroughput: 2_000_000, uploadThroughput: 1_000_000 });
      await v.page.reload();
      await v.up();
      const times = (await v.page.evaluate("({ firstFrame: pocketRequiem.firstFrame, firstGame: pocketRequiem.firstGame })")) as { firstFrame: number; firstGame: number };
      report.slowLine = { firstFrameMs: Math.round(times.firstFrame), firstGameMs: Math.round(times.firstGame) };
      await context.close();
    }
    // What the page told its host: once a visit, with the device it opened as (a host that names an address).
    report.opened = deployed ? { heard: opens.length } : { heard: opens.length, first: opens[0]?.query ?? "" };
  } finally {
    await browser.close();
  }
  return report;
}
