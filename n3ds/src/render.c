/* The PICA200 renderer.
 *
 * One pass with a 24-bit depth buffer: the sky, the ground and the props, the
 * mage and the demon, the army (meshes, then the far ranks), the shadows and
 * the effects, then the interface. The fragment stage is fixed-function: atlas
 * texel x vertex colour x 2, then the haze from a fog table. Vertex shaders
 * are in the .v.pica files.
 *
 * A knight is one draw: two stored frames bound as two buffers, blended by
 * the vertex program. The ground's patches, the far ranks, the effects'
 * vertices and the interface are written by the core (core.h); what it writes
 * every frame goes into one of two copies, so the copy the GPU still reads
 * from the frame before is left alone.
 *
 * Buffers the GPU reads are in linear memory. */
#include "render.h"

#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "color_shbin.h"
#include "crowd_shbin.h"
#include "hud_shbin.h"
#include "lit_shbin.h"
#include "skin_shbin.h"
#include "world_shbin.h"

#define HUD_QUADS 320
/* The shared quad indices: the interface, the stars, the far figures, a cohort's ranks. */
#define QUADS 1024
/* Effect vertices and indices a frame may hold: past them the farthest effects are left out. */
#define FX_VERTS 2304
#define FX_INDICES 4608
#define SKY_RADIUS 1000.0f
/* requiem_pack: metres per position unit of a static vertex. */
#define PICA_STEP (1.0f / 24.0f)
#define PICA_BACKDROP_STEP 0.25f
#define KIND_BACKDROP 3
/* The baked ground and props under a spell's light: sqrt(1 + cast x gain), the reference's rule. */
static const float LIGHT_GAIN[3] = {4.0f, 2.8f, 1.43f};

typedef struct {
  DVLB_s *dvlb;
  shaderProgram_s program;
  C3D_AttrInfo attr;
  int projection, extra;
} Program;

typedef struct {
  uint32_t bone_count;
  uint8_t bones[20];
  const uint8_t *vtx;
  const uint16_t *idx;
  uint32_t idx_count;
} Batch;

typedef struct {
  Batch batch[12];
  uint32_t batches;
} Model;

/* One kind of knight at one level of detail. */
typedef struct {
  uint32_t vtx_count, idx_count;
  const uint8_t *color, *frames;
  const uint16_t *idx;
} KnightMesh;

static Program world_prog, lit_prog, color_prog, skin_prog, crowd_prog, hud_prog;
static int skin_bones, skin_light, skin_cast, lit_cast, crowd_place, crowd_turn, crowd_light, crowd_cast;
static C3D_Tex pages[4], font_tex, fx_tex;
static unsigned page_count;
static C3D_FogLut fog_lut;
static const RqScene *scene;
static const RqMesh *meshes;
static const uint8_t *world_vtx;
static const uint16_t *world_idx;
static Model mage, demon;
static KnightMesh knights[3 * 8];
static uint32_t crowd_lods;
static float crowd_scale;
static const uint8_t *ground_mem;
static const RqColorVertex *ranks_mem;
static uint32_t ground_page;
/* Static geometry. */
static RqColorVertex *sky_vb, *star_vb, *moon_vb;
static uint16_t *sky_ib, *moon_ib, *quad_ib, *fan_ib, *ground_ib[2];
/* Two copies of what changes every frame. */
static RqColorVertex *dynamic_vb[2];
static RqHudVertex *hud_vb[2], *fx_vb[2];
static uint16_t *fx_ib[2];
static unsigned flip;
RenderStats render_stats;
int render_debug[4];

static bool program_init(Program *p, const u8 *shbin, u32 size, const char *extra) {
  p->dvlb = DVLB_ParseFile((u32 *)shbin, size);
  if (!p->dvlb)
    return false;
  shaderProgramInit(&p->program);
  shaderProgramSetVsh(&p->program, &p->dvlb->DVLE[0]);
  p->projection = shaderInstanceGetUniformLocation(p->program.vertexShader, "projection");
  p->extra = extra ? shaderInstanceGetUniformLocation(p->program.vertexShader, extra) : -1;
  AttrInfo_Init(&p->attr);
  return p->projection >= 0;
}

static int uniform(Program *p, const char *name) { return shaderInstanceGetUniformLocation(p->program.vertexShader, name); }

static void use(Program *p, const C3D_Mtx *projection) {
  C3D_BindProgram(&p->program);
  C3D_SetAttrInfo(&p->attr);
  C3D_FVUnifMtx4x4(GPU_VERTEX_SHADER, p->projection, projection);
}

static void buffer(const void *data, unsigned stride, int count, u64 permutation) {
  C3D_BufInfo *b = C3D_GetBufInfo();
  BufInfo_Init(b);
  BufInfo_Add(b, data, stride, count, permutation);
}

static u32 abgr(const float c[3], float a) {
  u32 v = 0;
  for (int i = 0; i < 3; i++) {
    float x = c[i] < 0 ? 0 : c[i] > 1 ? 1 : c[i];
    v |= (u32)(x * 255.0f + 0.5f) << (8 * i);
  }
  return v | ((u32)(a * 255.0f + 0.5f) << 24);
}

static void tev_color_only(void) {
  C3D_TexEnv *e = C3D_GetTexEnv(0);
  C3D_TexEnvInit(e);
  C3D_TexEnvSrc(e, C3D_Both, GPU_PRIMARY_COLOR, 0, 0);
  C3D_TexEnvFunc(e, C3D_Both, GPU_REPLACE);
}

/* texel x vertex colour, doubled: baked colours are stored at half scale. */
static void tev_world(void) {
  C3D_TexEnv *e = C3D_GetTexEnv(0);
  C3D_TexEnvInit(e);
  C3D_TexEnvSrc(e, C3D_Both, GPU_TEXTURE0, GPU_PRIMARY_COLOR, 0);
  C3D_TexEnvFunc(e, C3D_Both, GPU_MODULATE);
  C3D_TexEnvScale(e, C3D_RGB, GPU_TEVSCALE_2);
}

static void tev_hud(void) {
  C3D_TexEnv *e = C3D_GetTexEnv(0);
  C3D_TexEnvInit(e);
  C3D_TexEnvSrc(e, C3D_Both, GPU_TEXTURE0, GPU_PRIMARY_COLOR, 0);
  C3D_TexEnvFunc(e, C3D_Both, GPU_MODULATE);
}

/* An effect's texel is how bright it is: the colour is the vertex's, the strength the product. */
static void tev_fx(void) {
  C3D_TexEnv *e = C3D_GetTexEnv(0);
  C3D_TexEnvInit(e);
  C3D_TexEnvSrc(e, C3D_RGB, GPU_PRIMARY_COLOR, 0, 0);
  C3D_TexEnvFunc(e, C3D_RGB, GPU_REPLACE);
  C3D_TexEnvSrc(e, C3D_Alpha, GPU_TEXTURE0, GPU_PRIMARY_COLOR, 0);
  C3D_TexEnvFunc(e, C3D_Alpha, GPU_MODULATE);
}

static unsigned level_bytes(unsigned w, unsigned h) { return w * h * 2; }

static bool load_models(const uint8_t *m, char *error, size_t n) {
  uint32_t count;
  memcpy(&count, m, 4);
  size_t at = 4;
  for (uint32_t i = 0; i < count; i++) {
    uint32_t head[4];
    memcpy(head, m + at, 16);
    at += 16;
    Model *model = head[0] == 0 ? &mage : head[0] == 4 ? &demon : NULL;
    if (head[3] > 12) {
      snprintf(error, n, "a model has %lu draws", (unsigned long)head[3]);
      return false;
    }
    for (uint32_t b = 0; b < head[3]; b++) {
      uint32_t bh[4];
      memcpy(bh, m + at, 16);
      Batch batch = {.bone_count = bh[0], .vtx = m + at + 36, .idx = (const uint16_t *)(m + at + 36 + bh[1] * 24), .idx_count = bh[2]};
      memcpy(batch.bones, m + at + 16, 20);
      at = (at + 36 + bh[1] * 24 + bh[2] * 2 + 3) & ~(size_t)3;
      if (model)
        model->batch[model->batches++] = batch;
    }
  }
  if (!mage.batches) {
    snprintf(error, n, "the pack lacks the mage's model");
    return false;
  }
  return true;
}

bool render_init(const RenderData *d, char *error, size_t n) {
  scene = d->scene;
  meshes = d->meshes;
  world_vtx = d->vtx;
  world_idx = d->idx;
  ground_mem = d->ground_mem;
  ranks_mem = (const RqColorVertex *)d->ranks_mem;
  ground_page = d->ground_page;
  if (!program_init(&world_prog, world_shbin, world_shbin_size, "scale") || !program_init(&lit_prog, lit_shbin, lit_shbin_size, "scale") || !program_init(&color_prog, color_shbin, color_shbin_size, NULL) ||
      !program_init(&skin_prog, skin_shbin, skin_shbin_size, "bones") || !program_init(&crowd_prog, crowd_shbin, crowd_shbin_size, "place") || !program_init(&hud_prog, hud_shbin, hud_shbin_size, NULL)) {
    snprintf(error, n, "a shader program did not load");
    return false;
  }
  skin_bones = skin_prog.extra;
  skin_light = uniform(&skin_prog, "light");
  skin_cast = uniform(&skin_prog, "cast");
  lit_cast = uniform(&lit_prog, "cast");
  crowd_place = crowd_prog.extra;
  crowd_turn = uniform(&crowd_prog, "turn");
  crowd_light = uniform(&crowd_prog, "light");
  crowd_cast = uniform(&crowd_prog, "cast");
  render_debug[0] = skin_bones;
  render_debug[1] = skin_cast;
  render_debug[2] = crowd_turn;
  render_debug[3] = crowd_cast;
  /* World: position i16 x 4, texture coordinates i16 x 2, colour u8 x 4. In the buffer: uv, colour, position. */
  for (Program *p = &world_prog; p; p = p == &world_prog ? &lit_prog : NULL) {
    AttrInfo_AddLoader(&p->attr, 0, GPU_SHORT, 4);
    AttrInfo_AddLoader(&p->attr, 1, GPU_SHORT, 2);
    AttrInfo_AddLoader(&p->attr, 2, GPU_UNSIGNED_BYTE, 4);
  }
  /* Colour: position float x 3, colour u8 x 4. In the buffer: colour, position. */
  AttrInfo_AddLoader(&color_prog.attr, 0, GPU_FLOAT, 3);
  AttrInfo_AddLoader(&color_prog.attr, 1, GPU_UNSIGNED_BYTE, 4);
  /* Skin: position, normal i8 x 4, colour, bones and weights u8 x 4, in that order. */
  AttrInfo_AddLoader(&skin_prog.attr, 0, GPU_FLOAT, 3);
  AttrInfo_AddLoader(&skin_prog.attr, 1, GPU_BYTE, 4);
  AttrInfo_AddLoader(&skin_prog.attr, 2, GPU_UNSIGNED_BYTE, 4);
  AttrInfo_AddLoader(&skin_prog.attr, 3, GPU_UNSIGNED_BYTE, 4);
  /* The army: a frame's position i16 x 4 and normal i8 x 4, twice, then the colour. */
  AttrInfo_AddLoader(&crowd_prog.attr, 0, GPU_SHORT, 4);
  AttrInfo_AddLoader(&crowd_prog.attr, 1, GPU_BYTE, 4);
  AttrInfo_AddLoader(&crowd_prog.attr, 2, GPU_SHORT, 4);
  AttrInfo_AddLoader(&crowd_prog.attr, 3, GPU_BYTE, 4);
  AttrInfo_AddLoader(&crowd_prog.attr, 4, GPU_UNSIGNED_BYTE, 4);
  /* Interface and effects: position float x 3, texture coordinates float x 2, colour. In the buffer: uv, colour, position. */
  AttrInfo_AddLoader(&hud_prog.attr, 0, GPU_FLOAT, 3);
  AttrInfo_AddLoader(&hud_prog.attr, 1, GPU_FLOAT, 2);
  AttrInfo_AddLoader(&hud_prog.attr, 2, GPU_UNSIGNED_BYTE, 4);

  /* The atlas: every page's levels, already in the PICA's tiled layout. */
  page_count = scene->pages;
  if (page_count > 4 || d->tex_format != 5) {
    snprintf(error, n, "the atlas is not a 3DS texture");
    return false;
  }
  const uint8_t *src = d->tex;
  for (unsigned p = 0; p < page_count; p++) {
    C3D_TexInitParams params = {.width = d->tex_width, .height = d->tex_height, .maxLevel = d->tex_mips - 1, .format = GPU_RGB565, .type = GPU_TEX_2D, .onVram = true};
    if (!C3D_TexInitWithParams(&pages[p], NULL, params)) {
      snprintf(error, n, "no video memory for atlas page %u", p);
      return false;
    }
    for (unsigned l = 0; l < d->tex_mips; l++) {
      src = (const uint8_t *)(((uintptr_t)src + 15) & ~(uintptr_t)15);
      C3D_TexLoadImage(&pages[p], src, GPU_TEXFACE_2D, l);
      src += level_bytes(d->tex_width >> l, d->tex_height >> l);
    }
    C3D_TexSetFilter(&pages[p], GPU_LINEAR, GPU_LINEAR);
    C3D_TexSetFilterMipmap(&pages[p], GPU_NEAREST);
    C3D_TexSetWrap(&pages[p], GPU_REPEAT, GPU_CLAMP_TO_EDGE);
  }
  C3D_TexInitParams fp = {.width = d->font_width, .height = d->font_height, .maxLevel = 0, .format = GPU_RGBA4, .type = GPU_TEX_2D, .onVram = false};
  if (!C3D_TexInitWithParams(&font_tex, NULL, fp)) {
    snprintf(error, n, "no memory for the font texture");
    return false;
  }
  C3D_TexUpload(&font_tex, d->font_texels);
  C3D_TexSetFilter(&font_tex, GPU_NEAREST, GPU_NEAREST);
  C3D_TexSetWrap(&font_tex, GPU_CLAMP_TO_EDGE, GPU_CLAMP_TO_EDGE);
  /* The effects' atlas: 8 bits of strength a texel. */
  C3D_TexInitParams xp = {.width = d->fx_side, .height = d->fx_side, .maxLevel = 0, .format = GPU_A8, .type = GPU_TEX_2D, .onVram = false};
  if (d->fx_format != 8 || !C3D_TexInitWithParams(&fx_tex, NULL, xp)) {
    snprintf(error, n, "the effects' atlas is not a 3DS texture, or memory ran out");
    return false;
  }
  C3D_TexUpload(&fx_tex, d->fx_texels);
  C3D_TexSetFilter(&fx_tex, GPU_LINEAR, GPU_LINEAR);
  C3D_TexSetWrap(&fx_tex, GPU_CLAMP_TO_EDGE, GPU_CLAMP_TO_EDGE);

  if (!load_models(d->models, error, n))
    return false;

  /* The army: the table of meshes, then colours, indices and frames at the table's offsets. */
  uint32_t head[4];
  memcpy(head, d->crowd, 16);
  crowd_lods = head[1];
  memcpy(&crowd_scale, d->crowd + 12, 4);
  if (head[0] != 3 || crowd_lods > 8) {
    snprintf(error, n, "the pack's army has %lu kinds and %lu levels", (unsigned long)head[0], (unsigned long)crowd_lods);
    return false;
  }
  for (uint32_t i = 0; i < 3 * crowd_lods; i++) {
    uint32_t m[8];
    memcpy(m, d->crowd + 16 + i * 32, 32);
    knights[i] = (KnightMesh){.vtx_count = m[2], .idx_count = m[3], .color = d->crowd + m[4], .idx = (const uint16_t *)(d->crowd + m[5]), .frames = d->crowd + m[6]};
  }

  sky_vb = linearAlloc(RQ_SKY_VERTS * sizeof *sky_vb);
  star_vb = linearAlloc(RQ_STAR_VERTS * sizeof *star_vb);
  moon_vb = linearAlloc(RQ_MOON_VERTS * sizeof *moon_vb);
  sky_ib = linearAlloc(RQ_SKY_INDICES * 2);
  moon_ib = linearAlloc(RQ_MOON_INDICES * 2);
  quad_ib = linearAlloc(QUADS * 6 * 2);
  fan_ib = linearAlloc(RQ_DISC_INDICES * 2);
  ground_ib[0] = linearAlloc(RQ_GROUND_NEAR_INDICES * 2);
  ground_ib[1] = linearAlloc(RQ_GROUND_SMALL_INDICES * 2);
  for (int i = 0; i < 2; i++) {
    dynamic_vb[i] = linearAlloc((RQ_FAR_FIGURES * RQ_FAR_VERTS + RQ_DISC_VERTS) * sizeof(RqColorVertex));
    hud_vb[i] = linearAlloc(HUD_QUADS * 4 * sizeof(RqHudVertex));
    fx_vb[i] = linearAlloc(FX_VERTS * sizeof(RqHudVertex));
    fx_ib[i] = linearAlloc(FX_INDICES * 2);
  }
  if (!sky_vb || !star_vb || !moon_vb || !sky_ib || !moon_ib || !quad_ib || !fan_ib || !ground_ib[0] || !ground_ib[1] || !dynamic_vb[1] || !hud_vb[1] || !fx_vb[1] || !fx_ib[1]) {
    snprintf(error, n, "no linear memory for the frame buffers");
    return false;
  }
  /* The view is 58 degrees over 240 lines. */
  rq_static(SKY_RADIUS, 58.0f * (float)M_PI / 180.0f / 240.0f, sky_vb, sky_ib, star_vb, moon_vb, moon_ib, quad_ib, QUADS, fan_ib, ground_ib[0], ground_ib[1]);
  /* Haze: 1 - exp(-(depth x density)^2), as the reference computes it. */
  FogLut_Exp(&fog_lut, scene->fog_density, 2.0f, scene->clip_near, scene->clip_far);
  return true;
}

static void draw(GPU_Primitive_t prim, const uint16_t *idx, unsigned count) {
  C3D_DrawElements(prim, count, C3D_UNSIGNED_SHORT, idx);
  render_stats.draws++;
  render_stats.tris += count / 3;
}

static void fog(bool on, const float color[3]) {
  if (on) {
    C3D_FogGasMode(GPU_FOG, GPU_PLAIN_DENSITY, false);
    C3D_FogColor(abgr(color, 0) & 0xffffff);
    C3D_FogLutBind(&fog_lut);
  } else {
    C3D_FogGasMode(GPU_NO_FOG, GPU_PLAIN_DENSITY, false);
  }
}

/* The picks of one list on one atlas page. They arrive in the pack's order,
 * where meshes that share a vertex base follow each other with their indices
 * end to end: a run of them is one draw. */
static void picks(const Program *prog, unsigned list, unsigned count, unsigned page) {
  const RqPick *p = rq_picks(list);
  bool backdrop = false;
  uint32_t base = UINT32_MAX, first = 0, n = 0;
  for (unsigned i = 0; i <= count; i++) {
    const RqMesh *m = i < count ? &meshes[p[i].mesh] : NULL;
    if (m && m->page != page)
      continue;
    if (m && n && m->vtx_first == base && m->idx_first == first + n && (m->kind == KIND_BACKDROP) == backdrop) {
      n += m->idx_count;
      continue;
    }
    if (n) {
      buffer(world_vtx + (size_t)base * 16, 16, 3, 0x021);
      draw(GPU_TRIANGLES, world_idx + first, n);
    }
    if (!m)
      break;
    if ((m->kind == KIND_BACKDROP) != backdrop) {
      backdrop = m->kind == KIND_BACKDROP;
      C3D_FVUnifSet(GPU_VERTEX_SHADER, prog->extra, backdrop ? PICA_BACKDROP_STEP : PICA_STEP, 1.0f / 255.0f, scene->u_range / 32768.0f, 1.0f / 32768.0f);
    }
    base = m->vtx_first;
    first = m->idx_first;
    n = m->idx_count;
  }
  if (backdrop)
    C3D_FVUnifSet(GPU_VERTEX_SHADER, prog->extra, PICA_STEP, 1.0f / 255.0f, scene->u_range / 32768.0f, 1.0f / 32768.0f);
}

/* The spells' two strongest lights as a program takes them: place and 1 / radius^2, then colour. */
static void set_cast(int location, const RqView *view, const float gain[3]) {
  C3D_FVec *c = C3D_FVUnifWritePtr(GPU_VERTEX_SHADER, location, 4);
  for (int k = 0; k < 2; k++) {
    const RqCast *l = &view->cast[k];
    bool on = (unsigned)k < view->cast_count;
    c[k * 2] = FVec4_New(l->pos[0], l->pos[1], l->pos[2], on ? l->inv_r2 : 1.0f);
    c[k * 2 + 1] = on ? FVec4_New(l->color[0] * gain[0], l->color[1] * gain[1], l->color[2] * gain[2], 0.0f) : FVec4_New(0, 0, 0, 0);
  }
}

static void set_light(int location, const RqView *view) {
  C3D_FVec *l = C3D_FVUnifWritePtr(GPU_VERTEX_SHADER, location, 4);
  for (int i = 0; i < 4; i++)
    l[i] = FVec4_New(view->light[i * 4], view->light[i * 4 + 1], view->light[i * 4 + 2], view->light[i * 4 + 3]);
}

static void skinned(const Model *model, unsigned which) {
  static float rows[RQ_BATCH_BONES * 12];
  for (uint32_t k = 0; k < model->batches; k++) {
    const Batch *b = &model->batch[k];
    rq_bones(which, b->bones, b->bone_count, rows);
    C3D_FVec *u = C3D_FVUnifWritePtr(GPU_VERTEX_SHADER, skin_bones, b->bone_count * 3);
    for (uint32_t i = 0; i < b->bone_count * 3; i++)
      u[i] = FVec4_New(rows[i * 4], rows[i * 4 + 1], rows[i * 4 + 2], rows[i * 4 + 3]);
    buffer(b->vtx, 24, 4, 0x3210);
    draw(GPU_TRIANGLES, b->idx, b->idx_count);
  }
}

void render_frame(C3D_RenderTarget *target, const RqView *view, const RqPerf *perf) {
  render_stats.draws = render_stats.tris = 0;
  flip ^= 1;
  static const float one[3] = {1.0f, 1.0f, 1.0f};
  /* The clear colour is RRGGBBAA. */
  u32 f = abgr(view->fog, 1);
  C3D_RenderTargetClear(target, C3D_CLEAR_ALL, ((f & 0xff) << 24) | ((f & 0xff00) << 8) | ((f & 0xff0000) >> 8) | 0xff, 0);
  C3D_FrameDrawOn(target);

  C3D_Mtx proj, look, vp;
  Mtx_PerspTilt(&proj, C3D_AngleFromDegrees(view->fov), C3D_AspectRatioTop, scene->clip_near, scene->clip_far, false);
  Mtx_LookAt(&look, FVec3_New(view->eye[0], view->eye[1], view->eye[2]), FVec3_New(view->eye[0] + view->look[0], view->eye[1] + view->look[1], view->eye[2] + view->look[2]), FVec3_New(0, 1, 0), false);
  Mtx_RotateZ(&look, -view->roll, false);
  Mtx_Multiply(&vp, &proj, &look);

  C3D_DepthMap(true, -1.0f, 0.0f);
  C3D_AlphaTest(false, GPU_ALWAYS, 0);
  C3D_CullFace(GPU_CULL_NONE);

  /* ---------------------------------------------------------------- sky, stars, moon */
  C3D_Mtx sky = vp;
  Mtx_Translate(&sky, view->eye[0], view->eye[1], view->eye[2], true);
  use(&color_prog, &sky);
  tev_color_only();
  fog(false, view->fog);
  C3D_DepthTest(false, GPU_ALWAYS, GPU_WRITE_COLOR);
  C3D_AlphaBlend(GPU_BLEND_ADD, GPU_BLEND_ADD, GPU_ONE, GPU_ZERO, GPU_ONE, GPU_ZERO);
  buffer(sky_vb, 16, 2, 0x01);
  draw(GPU_TRIANGLES, sky_ib, RQ_SKY_INDICES);
  C3D_AlphaBlend(GPU_BLEND_ADD, GPU_BLEND_ADD, GPU_SRC_ALPHA, GPU_ONE_MINUS_SRC_ALPHA, GPU_ONE, GPU_ZERO);
  buffer(star_vb, 16, 2, 0x01);
  draw(GPU_TRIANGLES, quad_ib, RQ_STARS * 6);
  buffer(moon_vb, 16, 2, 0x01);
  draw(GPU_TRIANGLES, moon_ib, RQ_MOON_INDICES);
  C3D_AlphaBlend(GPU_BLEND_ADD, GPU_BLEND_ADD, GPU_ONE, GPU_ZERO, GPU_ONE, GPU_ZERO);

  /* ---------------------------------------------------------------- ground and props */
  C3D_DepthTest(true, GPU_GREATER, GPU_WRITE_ALL);
  fog(true, view->fog);
  GPU_CULLMODE cull = view->option & 1 ? GPU_CULL_NONE : view->option & 2 ? GPU_CULL_FRONT_CCW : GPU_CULL_BACK_CCW;
  if (view->world) {
    /* Under a spell's light the lit program; otherwise the plain one, which is shorter. */
    Program *prog = view->cast_count ? &lit_prog : &world_prog;
    use(prog, &vp);
    C3D_FVUnifSet(GPU_VERTEX_SHADER, prog->extra, PICA_STEP, 1.0f / 255.0f, scene->u_range / 32768.0f, 1.0f / 32768.0f);
    if (view->cast_count)
      set_cast(lit_cast, view, LIGHT_GAIN);
    tev_world();
    C3D_CullFace(cull);
    const RqPatch *patch = rq_patches();
    for (unsigned r = 0; r < view->repeat; r++) {
      for (unsigned page = 0; page < page_count; page++) {
        C3D_TexBind(0, &pages[page]);
        if (page == ground_page) {
          for (unsigned i = 0; i < view->patch_count; i++) {
            const RqPatch *p = &patch[i];
            bool near = p->level == 0;
            const uint8_t *v = ground_mem + p->offset;
            if (p->built && r == 0)
              GSPGPU_FlushDataCache(v, (near ? RQ_GROUND_NEAR_VERTS : RQ_GROUND_SMALL_VERTS) * 16);
            buffer(v, 16, 3, 0x021);
            draw(GPU_TRIANGLES, ground_ib[near ? 0 : 1], near ? RQ_GROUND_NEAR_INDICES : RQ_GROUND_SMALL_INDICES);
          }
        }
        picks(prog, 1, view->near_count, page);
        picks(prog, 0, view->far_count, page);
      }
    }
  }

  /* ---------------------------------------------------------------- the mage and the demon */
  if (view->mage) {
    use(&skin_prog, &vp);
    set_light(skin_light, view);
    set_cast(skin_cast, view, one);
    tev_color_only();
    C3D_CullFace(cull);
    skinned(&mage, 0);
    if (view->demon && demon.batches)
      skinned(&demon, 1);
  }

  /* ---------------------------------------------------------------- the army */
  if (view->knight_count) {
    use(&crowd_prog, &vp);
    set_light(crowd_light, view);
    set_cast(crowd_cast, view, one);
    tev_color_only();
    C3D_CullFace(cull);
    const RqKnight *k = rq_knights();
    uint32_t bound = UINT32_MAX;
    const KnightMesh *m = NULL;
    for (unsigned i = 0; i < view->knight_count; i++, k++) {
      uint32_t key = (uint32_t)k->mesh << 16 | (uint32_t)k->a << 8 | k->b;
      if (key != bound) {
        bound = key;
        m = &knights[k->mesh];
        C3D_BufInfo *b = C3D_GetBufInfo();
        BufInfo_Init(b);
        BufInfo_Add(b, m->frames + (size_t)k->a * m->vtx_count * 12, 12, 2, 0x10);
        BufInfo_Add(b, m->frames + (size_t)k->b * m->vtx_count * 12, 12, 2, 0x32);
        BufInfo_Add(b, m->color, 4, 1, 0x4);
      }
      C3D_FVUnifSet(GPU_VERTEX_SHADER, crowd_place, k->pos[0], k->pos[1], k->pos[2], k->blend);
      C3D_FVUnifSet(GPU_VERTEX_SHADER, crowd_turn, k->turn[0], k->turn[1], crowd_scale * k->scale / 32767.0f, k->flash);
      draw(GPU_TRIANGLES, m->idx, m->idx_count);
    }
  }

  /* ---------------------------------------------------------------- the far ranks, the far figures, the shadows */
  RqColorVertex *figure = dynamic_vb[flip], *disc = figure + RQ_FAR_FIGURES * RQ_FAR_VERTS;
  use(&color_prog, &vp);
  tev_color_only();
  C3D_CullFace(GPU_CULL_NONE);
  const RqRank *rank = rq_ranks();
  for (unsigned i = 0; i < view->rank_count; i++) {
    const RqRank *d = &rank[i];
    const RqColorVertex *v = ranks_mem + d->first;
    if (d->built)
      GSPGPU_FlushDataCache(v, d->verts * sizeof *v);
    /* The cohort's march since its mesh was written is the draw's translation. */
    C3D_Mtx moved = vp;
    Mtx_Translate(&moved, d->offset[0], d->offset[1], d->offset[2], true);
    C3D_FVUnifMtx4x4(GPU_VERTEX_SHADER, color_prog.projection, &moved);
    buffer(v, 16, 2, 0x01);
    draw(GPU_TRIANGLES, quad_ib, d->verts / 4 * 6);
  }
  C3D_FVUnifMtx4x4(GPU_VERTEX_SHADER, color_prog.projection, &vp);
  uint32_t figures = rq_far_figures(figure, RQ_FAR_FIGURES * RQ_FAR_VERTS);
  if (figures) {
    GSPGPU_FlushDataCache(figure, figures * RQ_FAR_VERTS * sizeof *figure);
    buffer(figure, 16, 2, 0x01);
    draw(GPU_TRIANGLES, quad_ib, figures * (RQ_FAR_VERTS / 4) * 6);
  }
  uint32_t discs = rq_shadows(disc, RQ_DISC_VERTS);
  C3D_AlphaBlend(GPU_BLEND_ADD, GPU_BLEND_ADD, GPU_SRC_ALPHA, GPU_ONE_MINUS_SRC_ALPHA, GPU_ONE, GPU_ZERO);
  C3D_DepthTest(true, GPU_GREATER, GPU_WRITE_COLOR);
  if (discs) {
    GSPGPU_FlushDataCache(disc, discs * (RQ_FAN + 1) * sizeof *disc);
    buffer(disc, 16, 2, 0x01);
    draw(GPU_TRIANGLES, fan_ib, discs * RQ_FAN * 3);
  }

  /* ---------------------------------------------------------------- effects */
  if (view->fx) {
    RqBatches b;
    rq_fx(fx_vb[flip], FX_VERTS, fx_ib[flip], FX_INDICES, &b);
    if (b.over + b.add) {
      GSPGPU_FlushDataCache(fx_vb[flip], b.verts * sizeof(RqHudVertex));
      GSPGPU_FlushDataCache(fx_ib[flip], (b.over + b.add) * 2);
      use(&hud_prog, &vp);
      tev_fx();
      fog(false, view->fog);
      C3D_TexBind(0, &fx_tex);
      buffer(fx_vb[flip], 24, 3, 0x021);
      if (b.over)
        draw(GPU_TRIANGLES, fx_ib[flip], b.over);
      if (b.add) {
        C3D_AlphaBlend(GPU_BLEND_ADD, GPU_BLEND_ADD, GPU_SRC_ALPHA, GPU_ONE, GPU_ONE, GPU_ZERO);
        draw(GPU_TRIANGLES, fx_ib[flip] + b.over, b.add);
        C3D_AlphaBlend(GPU_BLEND_ADD, GPU_BLEND_ADD, GPU_SRC_ALPHA, GPU_ONE_MINUS_SRC_ALPHA, GPU_ONE, GPU_ZERO);
      }
    }
  }

  /* ---------------------------------------------------------------- interface */
  C3D_Mtx ortho;
  Mtx_OrthoTilt(&ortho, 0.0f, 400.0f, 240.0f, 0.0f, -1.0f, 1.0f, true);
  RqHudVertex *hv = hud_vb[flip];
  /* The pack stores a texture's rows bottom-up, which is where the PICA starts v: texel row y is at v = y / height. */
  const float uv_scale[2] = {1.0f / font_tex.width, 1.0f / font_tex.height}, uv_offset[2] = {0.0f, 0.0f};
  RqPerf p = *perf;
  p.draws = render_stats.draws;
  p.tris = render_stats.tris;
  unsigned quads = rq_hud(hv, HUD_QUADS * 4, uv_scale, uv_offset, &p);
  if (quads) {
    GSPGPU_FlushDataCache(hv, quads * 4 * sizeof *hv);
    use(&hud_prog, &ortho);
    tev_hud();
    fog(false, view->fog);
    C3D_CullFace(GPU_CULL_NONE);
    C3D_DepthTest(false, GPU_ALWAYS, GPU_WRITE_COLOR);
    C3D_AlphaBlend(GPU_BLEND_ADD, GPU_BLEND_ADD, GPU_SRC_ALPHA, GPU_ONE_MINUS_SRC_ALPHA, GPU_ONE, GPU_ZERO);
    C3D_TexBind(0, &font_tex);
    buffer(hv, 24, 3, 0x021);
    draw(GPU_TRIANGLES, quad_ib, quads * 6);
  }
  C3D_AlphaBlend(GPU_BLEND_ADD, GPU_BLEND_ADD, GPU_ONE, GPU_ZERO, GPU_ONE, GPU_ZERO);
}
