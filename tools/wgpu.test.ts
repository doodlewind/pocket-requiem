// The browser version's wiring: the page offers the screens the renderer has, with the keys and the
// second screen each device has, and the listing's words name takes the recorder has.
import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";

const ROOT = resolve(import.meta.dir, "..");
const read = (file: string) => readFileSync(join(ROOT, file), "utf8");

test("the page shows the renderer's shapes, and no other device", () => {
  const shapes = [...read("wgpu/src/app.rs").matchAll(/Shape \{ name: "(\w+)", width: (\d+), height: (\d+)/g)].map((m) => m[1]);
  const devices = [...read("wgpu/page/main.js").matchAll(/\{ id: "(\w+)", label: "([^"]+)", sticks: (\d), glyphs: "(\w+)"/g)].map((m) => ({ id: m[1], sticks: Number(m[3]), glyphs: m[4] }));
  expect(shapes).toEqual(["vita", "psp", "3ds"]);
  expect(devices.map((d) => d.id)).toEqual(shapes);
  // Two sticks on the PS Vita; the others turn the eye with the direction pad. Letters on the 3DS's buttons.
  expect(devices.map((d) => d.sticks)).toEqual([2, 1, 1]);
  expect(devices.map((d) => d.glyphs)).toEqual(["playstation", "playstation", "letters"]);
  // The page names the packages' targets the game has: the three consoles.
  expect(read("wgpu/page/main.js")).toContain('runsOn: ["psp", "vita", "3ds"]');
});

test("the page plays the Pocket3D title card before it shows its canvas", () => {
  const page = read("wgpu/page/main.js");
  expect(page).toContain('import { playTitle } from "./pocket3d-title.js"');
  expect(page.indexOf("titleCard(playTitle)")).toBeGreaterThan(0);
  expect(page.indexOf("await title;\n  canvas.hidden = false;")).toBeGreaterThan(page.indexOf("titleCard(playTitle)"));
});

test("the listing's words are within the limits and every picture has a take", async () => {
  const run = Bun.spawnSync(["bun", "tools/listing.ts", "--check"], { cwd: ROOT, stdout: "pipe", stderr: "pipe" });
  expect(run.stderr.toString()).toBe("");
  expect(run.exitCode).toBe(0);
  const listing = JSON.parse(read("listing/listing.json"));
  expect(listing.media[0].kind).toBe("video");
  expect(listing.media.every((m: { from: string }) => m.from === "browser")).toBe(true);
});
