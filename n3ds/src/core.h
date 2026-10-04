/* The interface of core/ (Rust): the simulation and the device-independent
 * runtime. core/src/lib.rs defines these functions; rq_sizes lets main.c check
 * that the two sides agree on every layout. */
#ifndef REQUIEM_CORE_H
#define REQUIEM_CORE_H

#include <stddef.h>
#include <stdint.h>

#define RQ_BONES 28
/* Bones one draw of a skinned model holds: three uniform rows each. */
#define RQ_BATCH_BONES 19
#define RQ_SKY_VERTS 200
#define RQ_SKY_INDICES 1080
#define RQ_STARS 220
#define RQ_STAR_VERTS (RQ_STARS * 4)
#define RQ_MOON_VERTS 175
#define RQ_MOON_INDICES 504
#define RQ_FAN 8
#define RQ_DISCS 28
#define RQ_DISC_VERTS (RQ_DISCS * (RQ_FAN + 1))
#define RQ_DISC_INDICES (RQ_DISCS * RQ_FAN * 3)
#define RQ_FAR_VERTS 8
#define RQ_FAR_FIGURES 192
#define RQ_RANK_VERTS 8
#define RQ_GROUND_NEAR_INDICES 1920
#define RQ_GROUND_SMALL_INDICES 576
#define RQ_GROUND_NEAR_VERTS 353
#define RQ_GROUND_SMALL_VERTS 113
#define RQ_LIGHTS 4

/* Simulation buttons (requiem_sim::sim::btn) and the two the loop takes. */
#define RQ_LIGHT 1u
#define RQ_HEAVY 2u
#define RQ_EVADE 4u
#define RQ_UNSEAL 8u
#define RQ_GUARD 16u
#define RQ_HOVER 32u
#define RQ_START (1u << 16)
#define RQ_SELECT (1u << 17)

/* requiem_pack::HandScene */
typedef struct {
  float sun_dir[3], fog_density;
  float sun[3], lod_near;
  float sky[3], lod_mid;
  float bounce[3], lod_far;
  float fog[3], clip_near;
  float horizon[3], clip_far;
  float zenith[3], cell;
  float glow[3], super_cell;
  float fog_near, fog_far, moon_radius, spare;
  float u_range, color_scale, screen[2];
  uint32_t pages, near_streamed, crowd_lods, crowd_budget;
  float moon[3];
  uint32_t crowd_max;
  float crowd_reach[8];
} RqScene;

/* requiem_pack::HandMesh */
typedef struct {
  uint32_t kind;
  int32_t cx, cz;
  uint32_t page, vtx_first, vtx_count, idx_first, idx_count, big_first, clip_first;
  float clip_radius, min[3], max[3];
  uint32_t pad;
} RqMesh;

typedef struct {
  uint32_t bones, sky_verts, sky_indices, star_verts, moon_verts, moon_indices, disc_verts, disc_indices, fan, far_verts, far_figures, rank_verts, ground_near_indices, ground_small_indices, scene_bytes, mesh_bytes,
      vertex_bytes, knight_bytes, patch_bytes, view_bytes;
} RqSizes;

typedef struct {
  const RqScene *scene;
  const RqMesh *meshes;
  uint32_t mesh_count;
  const uint8_t *simw;
  uint32_t simw_len;
  const uint8_t *font;
  uint32_t font_len;
  const uint8_t *fxpk;
  uint32_t fxpk_len;
  const uint8_t *grnd;
  uint32_t grnd_len;
  const uint8_t *crowd;
  uint32_t crowd_len;
} RqPack;

typedef struct {
  uint32_t ground, ranks;
} RqMemory;

typedef struct {
  uint32_t buttons;
  float lx, ly, rx, ry;
} RqPad;

/* One light a spell casts: its place, 1 / radius^2, its linear colour times its power, its radius. */
typedef struct {
  float pos[3], inv_r2, color[3], radius;
} RqCast;

typedef struct {
  float eye[3], fov, look[3], roll, fog[3], impact;
  float light[16];
  RqCast cast[RQ_LIGHTS];
  uint32_t cast_count, world, mage, demon, fx, repeat;
  int32_t option;
  uint32_t far_count, near_count, patch_count, knight_count, rank_count;
} RqView;

typedef struct {
  uint32_t mesh, cell;
  float dist;
} RqPick;

/* requiem_handheld::ground::Patch */
typedef struct {
  uint32_t offset;
  uint8_t level, built, cx, cz;
  float min[3], max[3], dist;
} RqPatch;

/* requiem_handheld::crowd::Knight */
typedef struct {
  float pos[3], turn[2], blend, scale, flash, dist;
  uint16_t mesh;
  uint8_t a, b;
} RqKnight;

/* requiem_handheld::crowd::RankDraw */
typedef struct {
  uint32_t first, verts;
  float offset[3];
  uint32_t built;
} RqRank;

typedef struct {
  uint32_t verts, over, add, live;
} RqBatches;

/* Milliseconds, as the statistics line and the status record show them. */
typedef struct {
  float frame, worst;
  uint32_t late, frames;
  float sim, build, draw, gpu;
  uint32_t draws, tris;
} RqPerf;

/* Colour, then position. */
typedef struct {
  uint8_t color[4];
  float pos[3];
} RqColorVertex;

/* Texture coordinates, colour, position: the interface and the effects. */
typedef struct {
  float uv[2];
  uint8_t color[4];
  float pos[3];
} RqHudVertex;

typedef struct {
  float player[3], yaw, hp, mana;
  uint32_t kos, goal, standing, chain, ticks, auto_on, won;
  float demon[2];
} RqMap;

void rq_sizes(RqSizes *out);
/* Null, or a message. Every section may be freed afterwards except the army's. */
const char *rq_init(const RqPack *pack, RqMemory *need);
const char *rq_memory(uint8_t *ground_mem, uint8_t *ranks_mem);
uint32_t rq_font(const uint8_t *font, uint32_t font_len, uint32_t *width, uint32_t *height);
void rq_static(float radius, float pixel, RqColorVertex *sky_v, uint16_t *sky_i, RqColorVertex *star_v, RqColorVertex *moon_v, uint16_t *moon_i, uint16_t *quad_i, uint32_t quads, uint16_t *fan_i, uint16_t *ground_near_i,
               uint16_t *ground_small_i);
void rq_control(const char *text, uint32_t len);
uint32_t rq_step(const RqPad *pad, uint32_t ticks);
void rq_view(RqView *out);
const RqPick *rq_picks(uint32_t list);
const RqPatch *rq_patches(void);
const RqKnight *rq_knights(void);
const RqRank *rq_ranks(void);
uint32_t rq_far_figures(RqColorVertex *verts, uint32_t cap);
uint32_t rq_shadows(RqColorVertex *verts, uint32_t cap);
void rq_fx(RqHudVertex *verts, uint32_t vert_cap, uint16_t *idx, uint32_t idx_cap, RqBatches *out);
void rq_bones(uint32_t demon, const uint8_t *bones, uint32_t count, float *rows);
uint32_t rq_hud(RqHudVertex *verts, uint32_t cap, const float *uv_scale, const float *uv_offset, const RqPerf *perf);
void rq_audio(int16_t *out, uint32_t frames, float rate);
uint32_t rq_status(char *out, uint32_t cap, const RqPerf *perf, const char *extra, uint32_t extra_len);
uint32_t rq_map(RqMap *out, float *cohort_xz, uint32_t cap);

#endif
