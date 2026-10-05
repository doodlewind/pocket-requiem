/* Pocket Requiem on Nintendo 3DS.
 *
 * The same simulation as the reference (core/, Rust) and the same stage,
 * lowered by the stage compiler for this machine (profiles/n3ds30.json):
 * 400 x 240 on the upper screen at 30 frames a second, the PICA200's fixed
 * fragment stage, one Circle Pad.
 *
 * Controls: the Circle Pad moves; the +Control Pad (or a C-Stick) turns the
 * camera, which follows her when left alone; Y strikes, X casts, B evades,
 * A undoes the binding when her mana is full, L guards, R hovers. START
 * switches the autopilot, SELECT starts again. L + R + START leaves.
 *
 * Development loop over PocketJS's paired LAN wire (port 8131, compiled in
 * from vendor/pocketjs/hosts/3ds): {"t":"requiem.control","text":"..."}
 * steers the run and is answered with a requiem.status record; "screenshot"
 * captures both screens; a .3dsx install replaces this program. */
#include <3ds.h>
#include <citro3d.h>
#include <math.h>
#include <pocket3d_title.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

#include "core.h"
#include "devserver.h"
#include "hbldr.h"
#include "native.h"
#include "render.h"
#include "soc.h"

unsigned int __stacksize__ = 256 * 1024;
extern int __system_argc;
extern char **__system_argv;

#define TAG(a, b, c, d) ((u32)(a) | ((u32)(b) << 8) | ((u32)(c) << 16) | ((u32)(d) << 24))
#define AUDIO_RATE 11025
#define AUDIO_FRAMES 368
#define AUDIO_BUFFERS 4
/* Display refreshes a frame is given. */
#define PACE 2

static const u32 transfer = GX_TRANSFER_FLIP_VERT(0) | GX_TRANSFER_OUT_TILED(0) | GX_TRANSFER_RAW_COPY(0) | GX_TRANSFER_IN_FORMAT(GX_TRANSFER_FMT_RGBA8) | GX_TRANSFER_OUT_FORMAT(GX_TRANSFER_FMT_RGB8) |
                            GX_TRANSFER_SCALING(GX_TRANSFER_SCALE_NO);

static const char *stage = "boot";
static char app_error[256];
static FILE *boot_log;

static void flush_console(void) { GSPGPU_FlushDataCache(gfxGetFramebuffer(GFX_BOTTOM, GFX_LEFT, NULL, NULL), 320 * 240 * 2); }

static void say(const char *message) {
  printf("%s\n", message);
  flush_console();
  if (boot_log) {
    fprintf(boot_log, "%s\n", message);
    fflush(boot_log);
  }
  devserver_report_log("info", message);
}

/* ---------------------------------------------------------------- the pack */

typedef struct {
  u32 tag, offset, size, zero;
} Section;

static FILE *pack;
static Section sections[32];
static u32 section_count, pack_bytes;

static const Section *section(u32 tag) {
  for (u32 i = 0; i < section_count; i++)
    if (sections[i].tag == tag)
      return &sections[i];
  return NULL;
}

/* Reads a whole section into memory from `alloc` (malloc or linearAlloc). */
static void *read_section(u32 tag, void *(*alloc)(size_t), u32 *size) {
  const Section *s = section(tag);
  if (!s)
    return NULL;
  void *p = alloc(s->size ? s->size : 1);
  if (!p)
    return NULL;
  if (fseek(pack, s->offset, SEEK_SET) != 0 || fread(p, 1, s->size, pack) != s->size)
    return NULL;
  if (size)
    *size = s->size;
  return p;
}

static bool open_pack(void) {
  pack = fopen("romfs:/world.pack", "rb");
  if (!pack) {
    snprintf(app_error, sizeof app_error, "romfs:/world.pack is missing");
    return false;
  }
  u32 head[4];
  if (fread(head, 4, 4, pack) != 4 || head[0] != TAG('R', 'Q', 'P', 'K') || head[1] != 2 || head[2] > 32 || fread(sections, sizeof(Section), head[2], pack) != head[2]) {
    snprintf(app_error, sizeof app_error, "world.pack is not a version 2 pack");
    return false;
  }
  section_count = head[2];
  fseek(pack, 0, SEEK_END);
  pack_bytes = ftell(pack);
  return true;
}

static void *linear(size_t n) { return linearAlloc(n); }
static bool map_load(void);

static bool load(void) {
  RqSizes sizes;
  rq_sizes(&sizes);
  if (sizes.bones != RQ_BONES || sizes.sky_verts != RQ_SKY_VERTS || sizes.sky_indices != RQ_SKY_INDICES || sizes.star_verts != RQ_STAR_VERTS || sizes.moon_verts != RQ_MOON_VERTS || sizes.moon_indices != RQ_MOON_INDICES ||
      sizes.disc_verts != RQ_DISC_VERTS || sizes.disc_indices != RQ_DISC_INDICES || sizes.fan != RQ_FAN || sizes.far_verts != RQ_FAR_VERTS || sizes.far_figures != RQ_FAR_FIGURES || sizes.rank_verts != RQ_RANK_VERTS ||
      sizes.ground_near_indices != RQ_GROUND_NEAR_INDICES || sizes.ground_small_indices != RQ_GROUND_SMALL_INDICES || sizes.scene_bytes != sizeof(RqScene) || sizes.mesh_bytes != sizeof(RqMesh) || sizes.vertex_bytes != 16 ||
      sizes.knight_bytes != sizeof(RqKnight) || sizes.patch_bytes != sizeof(RqPatch) || sizes.view_bytes != sizeof(RqView)) {
    snprintf(app_error, sizeof app_error, "core.h does not match the core library");
    return false;
  }
  if (!open_pack())
    return false;
  static RenderData data;
  u32 scene_size = 0, mesh_size = 0, simw_size = 0, font_size = 0, tex_size = 0, fxpk_size = 0, grnd_size = 0, crowd_size = 0, fxtx_size = 0;
  say("Reading the field");
  RqScene *scene = read_section(TAG('H', 'S', 'C', 'N'), malloc, &scene_size);
  RqMesh *meshes = read_section(TAG('H', 'M', 'S', 'H'), malloc, &mesh_size);
  u8 *vtx = read_section(TAG('V', 'T', 'X', '0'), linear, NULL);
  u8 *idx = read_section(TAG('I', 'D', 'X', '0'), linear, NULL);
  u8 *models = read_section(TAG('M', 'O', 'D', 'L'), linear, NULL);
  say("Reading the army");
  u8 *crowd = read_section(TAG('C', 'R', 'W', 'D'), linear, &crowd_size);
  u8 *tex = read_section(TAG('T', 'E', 'X', '0'), linear, &tex_size);
  u8 *font = read_section(TAG('F', 'O', 'N', 'T'), linear, &font_size);
  u8 *fxtx = read_section(TAG('F', 'X', 'T', 'X'), linear, &fxtx_size);
  u8 *fxpk = read_section(TAG('F', 'X', 'P', 'K'), malloc, &fxpk_size);
  u8 *grnd = read_section(TAG('G', 'R', 'N', 'D'), malloc, &grnd_size);
  u8 *simw = read_section(TAG('S', 'I', 'M', 'W'), malloc, &simw_size);
  if (!scene || scene_size != sizeof(RqScene) || !meshes || !vtx || !idx || !models || !crowd || !tex || !font || !fxtx || !fxpk || !grnd || !simw || fxtx_size < 16) {
    snprintf(app_error, sizeof app_error, "world.pack lacks a section, or memory ran out (%lu KiB linear free)", (unsigned long)(linearSpaceFree() / 1024));
    return false;
  }
  say("Mustering");
  RqPack p = {.scene = scene,
              .meshes = meshes,
              .mesh_count = mesh_size / sizeof(RqMesh),
              .simw = simw,
              .simw_len = simw_size,
              .font = font,
              .font_len = font_size,
              .fxpk = fxpk,
              .fxpk_len = fxpk_size,
              .grnd = grnd,
              .grnd_len = grnd_size,
              .crowd = crowd,
              .crowd_len = crowd_size};
  RqMemory need = {0};
  const char *e = rq_init(&p, &need);
  u32 ground_page = 0;
  if (grnd_size >= 28)
    memcpy(&ground_page, grnd + 24, 4);
  free(simw);
  free(fxpk);
  free(grnd);
  u8 *ground_mem = e ? NULL : linearAlloc(need.ground), *ranks_mem = e ? NULL : linearAlloc(need.ranks);
  if (!e && (!ground_mem || !ranks_mem))
    e = "no linear memory for the ground and the far ranks";
  if (!e)
    e = rq_memory(ground_mem, ranks_mem);
  if (e) {
    snprintf(app_error, sizeof app_error, "%s", e);
    return false;
  }
  u32 font_w = 0, font_h = 0, font_at = rq_font(font, font_size, &font_w, &font_h);
  u32 th[4], xh[4];
  memcpy(th, tex, 16);
  memcpy(xh, fxtx, 16);
  data = (RenderData){.scene = scene,
                      .meshes = meshes,
                      .vtx = vtx,
                      .idx = (const uint16_t *)idx,
                      .models = models,
                      .crowd = crowd,
                      .ground_mem = ground_mem,
                      .ranks_mem = ranks_mem,
                      .ground_page = ground_page,
                      .tex = tex + 16,
                      .tex_width = th[0],
                      .tex_height = th[1],
                      .tex_mips = th[2],
                      .tex_format = th[3],
                      .font_texels = font + font_at,
                      .font_width = font_w,
                      .font_height = font_h,
                      .fx_texels = fxtx + 16,
                      .fx_side = xh[0],
                      .fx_format = xh[3]};
  say("Uploading textures");
  if (!font_at || !render_init(&data, app_error, sizeof app_error))
    return false;
  map_load();
  linearFree(tex);
  linearFree(font);
  linearFree(fxtx);
  fclose(pack);
  pack = NULL;
  return true;
}

/* ---------------------------------------------------------------- the lower screen */

/* The town from above (the pack's MAPT section), kept in the frame buffer's
 * own layout: columns left to right, each bottom-up. The left 240 columns of
 * the lower screen are one block of memory, so a redraw is one copy. */
static u16 *map_block;
static float map_extent;
static PrintConsole side;

static inline void plot(u16 *fb, int x, int y, u16 c) {
  if ((unsigned)x < 240 && (unsigned)y < 240)
    fb[x * 240 + (239 - y)] = c;
}

static void dot(u16 *fb, int x, int y, int r, u16 c, u16 edge) {
  for (int j = -r - 1; j <= r + 1; j++)
    for (int i = -r - 1; i <= r + 1; i++)
      plot(fb, x + i, y + j, abs(i) > r || abs(j) > r ? edge : c);
}

static bool map_load(void) {
  u32 size = 0;
  u8 *m = read_section(TAG('M', 'A', 'P', 'T'), malloc, &size);
  if (!m || size < 16 + 240 * 240 * 2)
    return false;
  u32 head[2];
  memcpy(head, m, 8);
  memcpy(&map_extent, m + 8, 4);
  map_block = malloc(240 * 240 * 2);
  if (head[0] != 240 || head[1] != 240 || !map_block)
    return false;
  const u16 *texel = (const u16 *)(m + 16);
  for (int x = 0; x < 240; x++)
    for (int y = 0; y < 240; y++)
      map_block[x * 240 + (239 - y)] = texel[y * 240 + x];
  free(m);
  return true;
}

/* The map, a mark for every cohort still in formation, the demon, and the
 * mage's position and heading. The marks change slowly, so the map with them
 * is kept (`map_marked`) and redrawn once a second; in between, an update
 * restores the patch under the mage's last mark and draws the new one: a few
 * hundred texels instead of 115 KiB. */
static u16 *map_marked;
static u32 map_drawn = UINT32_MAX;
static int map_px = -100, map_py = -100;
#define MARK_REACH 10

static void lower_map(void) {
  if (!map_block)
    return;
  RqMap map;
  static float cohorts[160 * 2];
  u32 n = rq_map(&map, cohorts, 160);
  u16 *fb = (u16 *)gfxGetFramebuffer(GFX_BOTTOM, GFX_LEFT, NULL, NULL);
  float k = 240.0f / (2.0f * map_extent);
  if (!map_marked)
    map_marked = malloc(240 * 240 * 2);
  if (!map_marked)
    return;
  int lo = 0, hi = 240;
  if (map.ticks / 60 != map_drawn || map.ticks < 30) {
    map_drawn = map.ticks / 60;
    memcpy(map_marked, map_block, 240 * 240 * 2);
    for (u32 i = 0; i < n; i++)
      dot(map_marked, (int)((cohorts[i * 2] + map_extent) * k), (int)((cohorts[i * 2 + 1] + map_extent) * k), 1, 0x9D7F, 0x0000);
    dot(map_marked, (int)((map.demon[0] + map_extent) * k), (int)((map.demon[1] + map_extent) * k), 2, 0xF81F, 0x0000);
    memcpy(fb, map_marked, 240 * 240 * 2);
  } else {
    /* Restore the columns around the last mark. */
    lo = map_px - MARK_REACH < 0 ? 0 : map_px - MARK_REACH;
    hi = map_px + MARK_REACH + 1 > 240 ? 240 : map_px + MARK_REACH + 1;
    if (lo < hi)
      memcpy(fb + lo * 240, map_marked + lo * 240, (hi - lo) * 240 * 2);
  }
  int px = (int)((map.player[0] + map_extent) * k), py = (int)((map.player[2] + map_extent) * k);
  /* The heading: a short line from the mark. */
  float dx = -sinf(map.yaw), dy = -cosf(map.yaw);
  for (int t = 3; t <= 8; t++) {
    plot(fb, px + (int)(dx * t), py + (int)(dy * t), 0xFFFF);
    plot(fb, px + (int)(dx * t) + 1, py + (int)(dy * t), 0x0000);
  }
  dot(fb, px, py, 2, 0xFFE0, 0x0000);
  map_px = px;
  map_py = py;
  int a = px - MARK_REACH < lo ? px - MARK_REACH : lo, b = px + MARK_REACH + 1 > hi ? px + MARK_REACH + 1 : hi;
  a = a < 0 ? 0 : a;
  b = b > 240 ? 240 : b;
  if (a < b)
    GSPGPU_FlushDataCache(fb + a * 240, (b - a) * 240 * 2);
}

/* Beside the map: the fight in numbers, and the frame's. Console text is drawn
 * in software, so the two halves go out on different frames. */
static void lower_text(const RqPerf *p, float gpu, bool run) {
  if (run) {
    RqMap map;
    rq_map(&map, NULL, 0);
    printf("\x1b[1;1HPOCKET\x1b[K\nREQUIEM\x1b[K\n\x1b[K\nHP %3.0f%%\x1b[K\nMANA %3.0f%%\x1b[K\n%lu / %lu\x1b[K\n%lu stand\x1b[K\n\x1b[K\n%s\x1b[K\n", map.hp * 100.0f, map.mana * 100.0f, (unsigned long)map.kos, (unsigned long)map.goal,
           (unsigned long)map.standing, map.won ? "UNDONE" : map.auto_on ? "AUTOPILOT" : "MANUAL");
  } else {
    printf("\x1b[12;1H%4.1f fps\x1b[K\n%4.1f ms\x1b[K\ngpu %4.1f\x1b[K\ncpu %4.1f\x1b[K\n%3.0fk tri\x1b[K\n%lu draw\x1b[K\n", 1000.0f / fmaxf(p->frame, 0.1f), p->frame, gpu, p->sim + p->build, p->tris / 1000.0f,
           (unsigned long)p->draws);
  }
  GSPGPU_FlushDataCache((u8 *)gfxGetFramebuffer(GFX_BOTTOM, GFX_LEFT, NULL, NULL) + 240 * 240 * 2, 80 * 240 * 2);
}

/* ---------------------------------------------------------------- sound */

static ndspWaveBuf wave[AUDIO_BUFFERS];
static bool sound;
static Result sound_result;
static void csnd_start_ring(void);

static void sound_init(void) {
  /* ndsp needs the console's DSP firmware dumped to sdmc:/3ds/dspfirm.cdc; without it there is no sound. */
  sound_result = ndspInit();
  if (R_FAILED(sound_result)) {
    csnd_start_ring();
    return;
  }
  ndspSetOutputMode(NDSP_OUTPUT_STEREO);
  ndspChnReset(0);
  ndspChnSetInterp(0, NDSP_INTERP_LINEAR);
  ndspChnSetRate(0, AUDIO_RATE);
  ndspChnSetFormat(0, NDSP_FORMAT_STEREO_PCM16);
  float mix[12] = {1.0f, 1.0f};
  ndspChnSetMix(0, mix);
  for (int i = 0; i < AUDIO_BUFFERS; i++) {
    wave[i].data_vaddr = linearAlloc(AUDIO_FRAMES * 4);
    wave[i].nsamples = AUDIO_FRAMES;
    wave[i].status = NDSP_WBUF_DONE;
    if (!wave[i].data_vaddr)
      return;
  }
  sound = true;
}

/* Without the DSP firmware dump, the older CSND service still plays a looping
 * buffer: the synthesizer writes ahead of the play position, which follows the
 * system clock at the channel's actual rate. One channel, the two sides mixed. */
#define CSND_RING 8192
#define CSND_LEAD 2048
#define CSND_CHANNEL 8
static s16 *csnd_ring;
static bool csnd_on;
static u64 csnd_start;
static u32 csnd_written;
static double csnd_rate;

static void csnd_start_ring(void) {
  if (R_FAILED(csndInit()))
    return;
  csnd_ring = linearAlloc(CSND_RING * 2);
  if (!csnd_ring)
    return;
  memset(csnd_ring, 0, CSND_RING * 2);
  GSPGPU_FlushDataCache(csnd_ring, CSND_RING * 2);
  /* The channel's timer divides the 67.03 MHz base clock by a whole number. */
  csnd_rate = 67027964.0 / (double)(u32)(67027964.0 / AUDIO_RATE);
  if (R_FAILED(csndPlaySound(CSND_CHANNEL, SOUND_FORMAT_16BIT | SOUND_REPEAT, AUDIO_RATE, 1.0f, 0.0f, csnd_ring, csnd_ring, CSND_RING * 2)))
    return;
  csnd_start = svcGetSystemTick();
  csnd_written = CSND_LEAD;
  csnd_on = true;
}

static void csnd_fill(void) {
  static s16 stereo[1024 * 2];
  u32 played = (u32)((double)(svcGetSystemTick() - csnd_start) * csnd_rate / SYSCLOCK_ARM11);
  /* Fell behind (a pause, a long load): start again ahead of the play position. */
  if ((s32)(csnd_written - played) < 256 || (s32)(csnd_written - played) > CSND_RING - 1024)
    csnd_written = played + CSND_LEAD / 2;
  s32 want = (s32)(played + CSND_LEAD - csnd_written);
  if (want <= 0)
    return;
  if (want > 1024)
    want = 1024;
  rq_audio(stereo, want, AUDIO_RATE);
  for (s32 i = 0; i < want; i++)
    csnd_ring[(csnd_written + i) % CSND_RING] = (s16)((stereo[i * 2] + stereo[i * 2 + 1]) / 2);
  csnd_written += want;
  GSPGPU_FlushDataCache(csnd_ring, CSND_RING * 2);
}

/* Refills at most two finished buffers a frame: 368 frames at 11.025 kHz is two display refreshes, one frame. */
static void sound_fill(void) {
  if (csnd_on) {
    csnd_fill();
    return;
  }
  if (!sound)
    return;
  int filled = 0;
  for (int i = 0; i < AUDIO_BUFFERS && filled < 2; i++) {
    if (wave[i].status != NDSP_WBUF_DONE)
      continue;
    rq_audio(wave[i].data_pcm16, AUDIO_FRAMES, AUDIO_RATE);
    DSP_FlushDataCache(wave[i].data_pcm16, AUDIO_FRAMES * 4);
    ndspChnWaveBufAdd(0, &wave[i]);
    filled++;
  }
}

/* ---------------------------------------------------------------- the wire */

static RqPerf perf;
static float cpu_ms, gpu_ms, wait_ms;
static u32 frame_no;
static bool new3ds;

static void report_status(void) {
  static char body[3072], extra[512], line[3200];
  int n = snprintf(extra, sizeof extra,
                   "\"build\":\"" REQUIEM_BUILD_ID "\",\"phase\":\"%s\",\"error\":\"%s\",\"packBytes\":%lu,\"linearFree\":%lu,\"vramFree\":%lu,\"cpuMsTotal\":%.3f,\"gpuWaitMs\":%.3f,\"sound\":%s,\"soundResult\":\"%08lx\",\"new3ds\":%s,\"uniforms\":[%d,%d,%d,%d]",
                   stage, app_error, (unsigned long)pack_bytes, (unsigned long)linearSpaceFree(), (unsigned long)vramSpaceFree(), cpu_ms, wait_ms, sound ? "\"ndsp\"" : csnd_on ? "\"csnd\"" : "false", (unsigned long)sound_result, new3ds ? "true" : "false", render_debug[0], render_debug[1], render_debug[2], render_debug[3]);
  if (!strcmp(stage, "running")) {
    rq_status(body, sizeof body, &perf, extra, n);
    /* {"target":... becomes {"t":"requiem.status","target":... */
    n = snprintf(line, sizeof line, "{\"t\":\"requiem.status\",%s", body + 1);
  } else {
    n = snprintf(line, sizeof line, "{\"t\":\"requiem.status\",\"target\":\"3ds\",\"stage\":\"%s\",%s}", stage, extra);
  }
  devserver_send_ctrl(line, n);
}

/* The "text" member of a control line: words for Game::control, no escapes. */
static void controls(const char *line) {
  if (!strstr(line, "\"requiem.control\""))
    return;
  const char *t = strstr(line, "\"text\":\"");
  if (t && !strcmp(stage, "running")) {
    t += 8;
    const char *end = strchr(t, '"');
    if (end)
      rq_control(t, end - t);
  }
  report_status();
}

static bool capture(C3D_RenderTarget *target) {
  uint8_t *top = NULL, *bottom = NULL;
  unsigned width = target->frameBuf.height, height = target->frameBuf.width;
  if (!devserver_screenshot_begin(frame_no, width, height, 320, 240, &top, &bottom))
    return false;
  C3D_SyncDisplayTransfer(target->frameBuf.colorBuf, GX_BUFFER_DIM(height, width), (u32 *)top, GX_BUFFER_DIM(height, width), transfer);
  GSPGPU_InvalidateDataCache(top, width * height * 3);
  u16 w, h;
  const uint16_t *fb = (const uint16_t *)gfxGetFramebuffer(GFX_BOTTOM, GFX_LEFT, &w, &h);
  if (w != 240 || h != 320) {
    devserver_screenshot_cancel();
    return false;
  }
  /* The console is RGB565; the wire carries BGR8 for both screens. */
  for (unsigned i = 0; i < 320 * 240; i++) {
    unsigned p = fb[i];
    bottom[3 * i] = (p & 31) * 255 / 31;
    bottom[3 * i + 1] = ((p >> 5) & 63) * 255 / 63;
    bottom[3 * i + 2] = ((p >> 11) & 31) * 255 / 31;
  }
  return true;
}

static void read_pad(RqPad *pad) {
  u32 held = hidKeysHeld();
  circlePosition c, cs;
  hidCircleRead(&c);
  hidCstickRead(&cs);
  pad->buttons = 0;
  if (held & KEY_Y)
    pad->buttons |= RQ_LIGHT;
  if (held & KEY_X)
    pad->buttons |= RQ_HEAVY;
  if (held & KEY_B)
    pad->buttons |= RQ_EVADE;
  if (held & KEY_A)
    pad->buttons |= RQ_UNSEAL;
  if (held & (KEY_L | KEY_ZL))
    pad->buttons |= RQ_GUARD;
  if (held & (KEY_R | KEY_ZR))
    pad->buttons |= RQ_HOVER;
  if (held & KEY_START)
    pad->buttons |= RQ_START;
  if (held & KEY_SELECT)
    pad->buttons |= RQ_SELECT;
  /* The Circle Pad reaches about 150 units; nothing inside a sixth of that. */
  float lx = fmaxf(-1.0f, fminf(1.0f, c.dx / 150.0f)), ly = fmaxf(-1.0f, fminf(1.0f, c.dy / 150.0f));
  pad->lx = fabsf(lx) < 0.16f ? 0.0f : lx;
  pad->ly = fabsf(ly) < 0.16f ? 0.0f : ly;
  pad->rx = (held & KEY_DRIGHT ? 0.8f : 0.0f) - (held & KEY_DLEFT ? 0.8f : 0.0f);
  pad->ry = (held & KEY_DUP ? 0.8f : 0.0f) - (held & KEY_DDOWN ? 0.8f : 0.0f);
  /* A C-Stick (New 3DS) reaches about 100 units. */
  if (abs(cs.dx) > 12 || abs(cs.dy) > 12) {
    pad->rx = fmaxf(-1.0f, fminf(1.0f, cs.dx / 100.0f));
    pad->ry = fmaxf(-1.0f, fminf(1.0f, cs.dy / 100.0f));
  }
}

int main(void) {
  gfxInitDefault();
  gfxSet3D(false);
  /* The Pocket3D title card, on both screens, before the GPU is set up. */
  pocket3d_title_play();
  gfxSetDoubleBuffering(GFX_BOTTOM, false);
  consoleInit(GFX_BOTTOM, NULL);
  mkdir("sdmc:/pocket-requiem", 0777);
  boot_log = fopen("sdmc:/pocket-requiem/boot.log", "w");
  say("Pocket Requiem " REQUIEM_BUILD_ID);
  char error[256] = {0};
  PocketRuntimeState state = {0};
  if (__system_argc > 0 && __system_argv)
    native_set_running_path(__system_argv[0]);
  mkdir("sdmc:/pocketjs", 0777);
  mkdir("sdmc:/pocketjs/runtime", 0777);
  devserver_allow_packages(false);
  DevserverInitResult dev = devserver_init(&state, error, sizeof error);
  say(dev == DEVSERVER_READY ? "Remote debugger ready" : error);
  /* A New 3DS runs at 804 MHz when asked; an Old 3DS ignores it. */
  osSetSpeedupEnable(true);
  APT_CheckNew3DS(&new3ds);

  bool gpu_ok = C3D_Init(C3D_DEFAULT_CMDBUF_SIZE * 2), gpu_stalled = false;
  C3D_RenderTarget *top = gpu_ok ? C3D_RenderTargetCreate(240, 400, GPU_RB_RGBA8, GPU_RB_DEPTH24_STENCIL8) : NULL;
  if (top)
    C3D_RenderTargetSetOutput(top, GFX_TOP, GFX_LEFT, transfer);
  Result romfs = romfsInit();
  bool loaded = false;
  stage = "loading";
  devserver_set_runtime(&state, NULL, stage, 0);
  devserver_poll();
  if (!gpu_ok || !top)
    snprintf(app_error, sizeof app_error, "PICA initialization failed");
  else if (R_FAILED(romfs))
    snprintf(app_error, sizeof app_error, "romfsInit failed: %08lx", (unsigned long)romfs);
  else
    loaded = load();
  if (loaded) {
    sound_init();
    stage = "running";
    consoleClear();
    /* The map takes the left 240 pixels of the lower screen; text keeps the ten columns beside it. */
    if (map_block) {
      consoleInit(GFX_BOTTOM, &side);
      /* Window coordinates count from 1. */
      consoleSetWindow(&side, 31, 1, 10, 30);
    }
  } else {
    stage = "load-error";
    say(app_error);
  }
  devserver_set_runtime(&state, NULL, stage, 0);

  u64 last = svcGetSystemTick(), next_retry = 0;
  u32 ticks = PACE, late = 0, last_count = C3D_FrameCounter(0);
  float worst = 0, avg = 33.3f;
  bool shot = false, capture_pending = false;
  while (aptMainLoop()) {
    hidScanInput();
    u32 held = hidKeysHeld();
    if ((held & (KEY_L | KEY_R | KEY_START)) == (KEY_L | KEY_R | KEY_START))
      break;
    if (!capture_pending)
      devserver_poll();
    u64 now = svcGetSystemTick();
    if (dev != DEVSERVER_READY && now >= next_retry) {
      dev = devserver_init(&state, error, sizeof error);
      next_retry = now + SYSCLOCK_ARM11 * 3ULL;
    }
    char launch[POCKET_RUNTIME_NATIVE_NAME_BYTES + 1], path[POCKET_NATIVE_PATH_BYTES];
    if (devserver_take_launch(launch) && native_path_for(launch, path)) {
      if (hbldr_launch_on_exit(path, error, sizeof error)) {
        devserver_flush(1000);
        devserver_report_native("launching", launch, "exiting to start it");
        devserver_poll();
        devserver_flush(1000);
        for (unsigned i = 0; i < 200; i++) {
          devserver_poll();
          svcSleepThread(1000000);
        }
        break;
      }
      devserver_report_native("launch-error", launch, error);
    }
    static char control[8192];
    size_t n;
    while ((n = devserver_recv_ctrl(control, sizeof control - 1)) > 0) {
      control[n] = 0;
      char *save = NULL;
      for (char *line = strtok_r(control, "\n", &save); line; line = strtok_r(NULL, "\n", &save)) {
        if (strstr(line, "\"screenshot\""))
          devserver_request_screenshot();
        controls(line);
      }
    }
    if (native_receiving() || gpu_stalled || !loaded) {
      svcSleepThread(1000000);
      continue;
    }
    float frame_ms = (float)(now - last) * 1000.0f / SYSCLOCK_ARM11;
    last = now;

    /* ------------------------------------------------------------ input and simulation */
    RqPad pad;
    read_pad(&pad);
    rq_step(&pad, ticks);
    sound_fill();
    RqView view;
    rq_view(&view);
    float sim_ms = (float)(svcGetSystemTick() - now) * 1000.0f / SYSCLOCK_ARM11;

    /* ------------------------------------------------------------ the previous frame leaves the GPU */
    u64 wait_start = svcGetSystemTick();
    while (!C3D_FrameBegin(C3D_FRAME_NONBLOCK)) {
      if (!capture_pending)
        devserver_poll();
      svcSleepThread(100000);
      if (svcGetSystemTick() - wait_start > SYSCLOCK_ARM11 * 4ULL) {
        stage = "gpu-timeout";
        say("GPU timeout: the debugger remains available");
        devserver_set_runtime(&state, NULL, stage, 0);
        gpu_stalled = true;
        break;
      }
    }
    if (gpu_stalled)
      continue;
    gpu_ms = C3D_GetDrawingTime();
    if (capture_pending) {
      devserver_screenshot_ready();
      capture_pending = false;
    }
    if (shot) {
      capture_pending = capture(top);
      shot = false;
    }
    /* A frame every PACE display refreshes: 30 frames a second. */
    while (C3D_FrameCounter(0) - last_count < PACE) {
      if (!capture_pending)
        devserver_poll();
      svcSleepThread(100000);
    }
    u64 build_start = svcGetSystemTick();
    wait_ms = (float)(build_start - wait_start) * 1000.0f / SYSCLOCK_ARM11;
    /* One tick per display refresh since the last frame: a late frame catches up. */
    u32 count = C3D_FrameCounter(0);
    ticks = count - last_count;
    last_count = count;
    if (ticks > PACE)
      late++;
    if (ticks < 1)
      ticks = 1;
    /* At most one tick of catching up: a slow frame slows the game, it does not ask the next frame for more. */
    if (ticks > PACE + 1)
      ticks = PACE + 1;
    avg += (frame_ms - avg) * 0.05f;
    worst = frame_no % 120 == 0 ? frame_ms : fmaxf(worst, frame_ms);
    perf = (RqPerf){.frame = avg, .worst = worst, .late = late, .frames = frame_no, .sim = sim_ms, .build = perf.build, .draw = 0, .gpu = gpu_ms, .draws = render_stats.draws, .tris = render_stats.tris};

    render_frame(top, &view, &perf);
    C3D_FrameEnd(0);
    perf.build = (float)(svcGetSystemTick() - build_start) * 1000.0f / SYSCLOCK_ARM11;
    perf.draws = render_stats.draws;
    perf.tris = render_stats.tris;
    cpu_ms = sim_ms + perf.build;
    frame_no++;
    devserver_set_runtime(&state, NULL, stage, frame_no);
    devserver_set_frame_stats(frame_no, render_stats.draws, render_stats.tris * 3, 0);
    devserver_set_frame_timing(0, (uint32_t)(sim_ms * 1000), (uint32_t)(perf.build * 1000), (uint32_t)(gpu_ms * 1000), (uint32_t)(frame_ms * 1000));
    shot = shot || devserver_take_screenshot_request();

    /* The map six times a second, the text twice, on different frames. */
    if (frame_no % 5 == 3)
      lower_map();
    if (frame_no % 15 == 7)
      lower_text(&perf, gpu_ms, true);
    if (frame_no % 30 == 19)
      lower_text(&perf, gpu_ms, false);
  }

  if (gpu_ok && !gpu_stalled) {
    /* Retire the frame in flight before the targets go. */
    for (unsigned i = 0; i < 4000 && !C3D_FrameBegin(C3D_FRAME_NONBLOCK); i++) {
      devserver_poll();
      svcSleepThread(1000000);
    }
    C3D_FrameEnd(0);
    if (top)
      C3D_RenderTargetDelete(top);
    C3D_Fini();
  }
  if (sound)
    ndspExit();
  if (csnd_on) {
    CSND_SetPlayState(CSND_CHANNEL, 0);
    csndExecCmds(true);
    csndExit();
  }
  if (!gpu_stalled || !capture_pending)
    devserver_shutdown();
  soc_shutdown();
  if (R_SUCCEEDED(romfs))
    romfsExit();
  if (native_exit_pending() && !native_finish_exit(error, sizeof error))
    say(error);
  if (boot_log)
    fclose(boot_log);
  gfxExit();
  return 0;
}
