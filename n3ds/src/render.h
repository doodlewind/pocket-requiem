#ifndef REQUIEM_RENDER_H
#define REQUIEM_RENDER_H

#include <3ds.h>
#include <citro3d.h>
#include <stdbool.h>

#include "core.h"

/* The pack's drawing data. Vertex, index, model and army buffers are in linear
 * memory and stay for the life of the program, as do the two blocks the core
 * writes the ground's patches and the far ranks into; the texture buffers may
 * be freed after render_init. */
typedef struct {
  const RqScene *scene;
  const RqMesh *meshes;
  const uint8_t *vtx;
  const uint16_t *idx;
  const uint8_t *models;
  /* The army: the pack's CRWD section. */
  const uint8_t *crowd;
  const uint8_t *ground_mem, *ranks_mem;
  /* The atlas page the ground's strip is on. */
  uint32_t ground_page;
  /* The atlas: after the header, every page's levels, each on a 16-byte boundary. */
  const uint8_t *tex;
  uint32_t tex_width, tex_height, tex_mips, tex_format;
  const uint8_t *font_texels;
  uint32_t font_width, font_height;
  /* The effects' atlas: one level of 8-bit texels. */
  const uint8_t *fx_texels;
  uint32_t fx_side, fx_format;
} RenderData;

typedef struct {
  uint32_t draws, tris;
} RenderStats;

extern RenderStats render_stats;
/* Uniform registers of the skinning and army programs, for the status record. */
extern int render_debug[4];

bool render_init(const RenderData *data, char *error, size_t n);
/* Draws one frame between C3D_FrameBegin and C3D_FrameEnd. */
void render_frame(C3D_RenderTarget *target, const RqView *view, const RqPerf *perf);

#endif
