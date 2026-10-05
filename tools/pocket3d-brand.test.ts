// The launcher art's rules, checked on the host: `bun test ./tools`.
// The procedure is vendor/pocketjs/skills/pocket3d-brand/SKILL.md.

import { expect, test } from "bun:test";
import { existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { POCKET3D_ICON } from "../vendor/pocketjs/tools/pocket3d-icon.ts";
import { resolveVitaPackageAssets, VITA_ICON_VPK_PATH } from "../vendor/pocketjs/tools/vita-package.ts";

const ROOT = resolve(import.meta.dir, "..");
const read = (path: string) => readFileSync(join(ROOT, path), "utf8");

test("the repository tracks no icon of its own", () => {
  const tracked = Bun.spawnSync(["git", "ls-files"], { cwd: ROOT }).stdout.toString().split("\n").filter((f) => f && !f.startsWith("vendor/"));
  expect(tracked).toContain("psp/Psp.toml");
  // icon0.png, ICON0.PNG, icon.png, icon-small.png, Icon@2x.png
  expect(tracked.filter((f) => /(^|\/)icon[^/]*\.png$/i.test(f))).toEqual([]);
  // The paths the builds read before: a file left there is picked up again by the next script that looks.
  for (const old of ["psp/assets/icon0.png", "vita/assets/sce_sys/icon0.png", "n3ds/icon.png"]) expect(existsSync(join(ROOT, old))).toBe(false);
});

test("each console's build reads the icon from the PocketJS checkout", () => {
  for (const file of Object.values(POCKET3D_ICON)) expect(existsSync(file)).toBe(true);
  expect(read("psp/Psp.toml")).toContain('xmb_icon_png = "../vendor/pocketjs/engine/pocket3d/icon/psp/ICON0.PNG"');
  expect(read("tools/vita.ts")).toContain("icon: POCKET3D_ICON.vita");
  const makefile = read("n3ds/Makefile");
  expect(makefile).toContain("ICON := $(ROOT)/vendor/pocketjs/engine/pocket3d/icon/3ds/icon.png\n");
  expect(makefile).toContain("SMALL_ICON := $(ROOT)/vendor/pocketjs/engine/pocket3d/icon/3ds/icon-small.png\n");
  // The large icon, the output, the small icon: given one icon, smdhtool halves the large one.
  expect(makefile).toMatch(/smdhtool --create .* \$\(ICON\) \$@ \$\(SMALL_ICON\)\n/);
  expect(read("tools/n3ds.ts")).not.toContain("icon.png");
});

test("the VPK holds the Pocket3D icon and this repository's LiveArea pictures", () => {
  // Throws unless bg.png is 840x500 and startup.png 280x158, both 8-bit indexed.
  const assets = resolveVitaPackageAssets({ applicationAssets: join(ROOT, "vita/assets"), icon: POCKET3D_ICON.vita });
  const source = (destination: string) => assets.find((a) => a.destination === destination)?.source;
  expect(source(VITA_ICON_VPK_PATH)).toBe(POCKET3D_ICON.vita);
  const livearea = "sce_sys/livearea/contents";
  for (const name of ["bg.png", "startup.png", "template.xml"]) expect(source(`${livearea}/${name}`)).toBe(join(ROOT, "vita/assets", livearea, name));
});
