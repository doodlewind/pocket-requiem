//! Interface drawing in the screen's pixels: solid rectangles and text from
//! the pack's glyph atlas, batched into one draw (`vita/src/hud.rs`).
//!
//! The glyphs are the PS Vita's, cut for 960 × 544. On a smaller screen a
//! batch draws them at `scale`, the screen's height over 544, and the atlas
//! has the levels below its own for that.

use std::rc::Rc;

use pocket_web_wgpu::gpu::Gpu;
use pocket_web_wgpu::wgpu;
use requiem_pack::{self as pack, FontHeader, Glyph, Pack};

pub const MAX_QUADS: usize = 1024;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pos: [f32; 2],
    uv: [f32; 2],
    color: [u8; 4],
}

/// The glyph table of the pack's `FONT` section.
pub struct Font {
    glyphs: Vec<Glyph>,
    sizes: Vec<u16>,
    /// Texture coordinates of the atlas's solid block.
    solid: [f32; 2],
    w: f32,
    h: f32,
}

impl Font {
    /// The table, and the atlas's coverage as rows of bytes with its last 2 × 2 texels solid.
    fn parse(b: &[u8]) -> Result<(Font, Vec<u8>), String> {
        let head: FontHeader = pack::read(b, 0).ok_or("font header")?;
        let gsize = core::mem::size_of::<Glyph>();
        let table = core::mem::size_of::<FontHeader>();
        let glyphs: Vec<Glyph> = (0..head.glyphs as usize).filter_map(|i| pack::read(b, table + i * gsize)).collect();
        let (w, h) = (head.width as usize, head.height as usize);
        let from = table + head.glyphs as usize * gsize;
        if head.pad != 0 || glyphs.len() != head.glyphs as usize || b.len() < from + w * h || w < 2 || h < 2 {
            return Err("the pack's font is not 8-bit coverage".into());
        }
        let mut cover = b[from..from + w * h].to_vec();
        for (x, y) in [(w - 1, h - 1), (w - 2, h - 1), (w - 1, h - 2), (w - 2, h - 2)] {
            cover[y * w + x] = 255;
        }
        let mut sizes: Vec<u16> = glyphs.iter().map(|g| g.size).collect();
        sizes.dedup();
        Ok((Font { glyphs, sizes, solid: [(w as f32 - 1.0) / w as f32, (h as f32 - 1.0) / h as f32], w: w as f32, h: h as f32 }, cover))
    }

    fn glyph(&self, size: u16, code: u8) -> Option<&Glyph> {
        let s = self.sizes.iter().position(|&x| x == size)?;
        self.glyphs.get(s * 95 + (code.clamp(32, 126) - 32) as usize)
    }

    /// The width of `text` at `size`, in the atlas's own pixels.
    pub fn width(&self, size: u16, text: &str) -> f32 {
        text.bytes().filter_map(|c| self.glyph(size, c)).map(|g| g.advance).sum()
    }
}

pub fn rgba(r: u8, g: u8, b: u8, a: u8) -> [u8; 4] {
    [r, g, b, a]
}

/// One frame's batch.
pub struct Hud {
    font: Rc<Font>,
    /// Screen pixels to one pixel of a glyph.
    pub scale: f32,
    verts: Vec<Vertex>,
}

impl Hud {
    pub fn new(font: Rc<Font>, scale: f32) -> Hud {
        Hud { font, scale, verts: Vec::with_capacity(1024) }
    }

    #[allow(clippy::too_many_arguments)]
    fn quad(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, u0: f32, v0: f32, u1: f32, v1: f32, color: [u8; 4]) {
        if self.verts.len() >= MAX_QUADS * 4 {
            return;
        }
        self.verts.extend_from_slice(&[Vertex { pos: [x0, y0], uv: [u0, v0], color }, Vertex { pos: [x1, y0], uv: [u1, v0], color }, Vertex { pos: [x1, y1], uv: [u1, v1], color }, Vertex { pos: [x0, y1], uv: [u0, v1], color }]);
    }

    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: [u8; 4]) {
        let [u, v] = self.font.solid;
        self.quad(x, y, x + w, y + h, u, v, u, v, color)
    }

    /// A rectangle outline `t` pixels thick.
    pub fn frame(&mut self, x: f32, y: f32, w: f32, h: f32, t: f32, color: [u8; 4]) {
        self.rect(x, y, w, t, color);
        self.rect(x, y + h - t, w, t, color);
        self.rect(x, y + t, t, h - t * 2.0, color);
        self.rect(x + w - t, y + t, t, h - t * 2.0, color);
    }

    /// The width of `text` at `size` on this screen.
    pub fn width(&self, size: u16, text: &str) -> f32 {
        self.font.width(size, text) * self.scale
    }

    /// Text with its baseline at `y`. `align`: 0 left, 0.5 centre, 1 right of `x`.
    pub fn text(&mut self, size: u16, x: f32, y: f32, align: f32, color: [u8; 4], text: &str) {
        let k = self.scale;
        let pen = (x - self.width(size, text) * align).round();
        let shadow = [0, 0, 0, (color[3] as u32 * 150 / 255) as u8];
        for pass in 0..2 {
            let mut px = pen;
            for c in text.bytes() {
                let Some(gl) = self.font.glyph(size, c).copied() else { continue };
                if gl.w > 0 {
                    let (o, col) = if pass == 0 { (1.5 * k, shadow) } else { (0.0, color) };
                    let x0 = px + gl.left as f32 * k + o;
                    let y0 = y - gl.top as f32 * k + o;
                    let (u0, v0) = (gl.x as f32 / self.font.w, gl.y as f32 / self.font.h);
                    let (u1, v1) = ((gl.x + gl.w) as f32 / self.font.w, (gl.y + gl.h) as f32 / self.font.h);
                    self.quad(x0, y0, x0 + gl.w as f32 * k, y0 + gl.h as f32 * k, u0, v0, u1, v1, col)
                }
                px += gl.advance * k;
            }
        }
    }
}

/// The batch on the GPU.
pub struct Painter {
    pub font: Rc<Font>,
    pipeline: wgpu::RenderPipeline,
    group: wgpu::BindGroup,
    screen: wgpu::Buffer,
    verts: wgpu::Buffer,
    indices: wgpu::Buffer,
    quads: u32,
}

impl Painter {
    pub fn load(gpu: &Gpu, p: &Pack, format: wgpu::TextureFormat) -> Result<Painter, String> {
        Self::from_font(gpu, p.section(pack::FONT)?, format)
    }

    /// From the bytes of a `FONT` section.
    pub fn from_font(gpu: &Gpu, section: &[u8], format: wgpu::TextureFormat) -> Result<Painter, String> {
        let (font, cover) = Font::parse(section)?;
        let (w, h) = (font.w as u32, font.h as u32);
        let mut levels = vec![cover];
        while levels.len() < 4 {
            let level = levels.len() - 1;
            let next = crate::render::halve(&levels[level], (w >> level) as usize, (h >> level) as usize);
            levels.push(next);
        }
        let device = &gpu.device;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("font"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: levels.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (level, texels) in levels.iter().enumerate() {
            let (lw, lh) = (w >> level, h >> level);
            gpu.queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: level as u32, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                texels,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(lw), rows_per_image: None },
                wgpu::Extent3d { width: lw, height: lh, depth_or_array_layers: 1 },
            );
        }
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor { label: Some("font"), mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, mipmap_filter: wgpu::FilterMode::Linear, ..Default::default() });
        let uniform = |binding| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::VERTEX, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: wgpu::BufferSize::new(16) }, count: None };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hud"),
            entries: &[
                uniform(0),
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let screen = device.create_buffer(&wgpu::BufferDescriptor { label: Some("hud screen"), size: 16, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hud"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: screen.as_entire_binding() }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view) }, wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&sampler) }],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("hud"), source: wgpu::ShaderSource::Wgsl(include_str!("shaders/hud.wgsl").into()) });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("hud"), bind_group_layouts: &[&layout], push_constant_ranges: &[] });
        let over = |src| wgpu::BlendComponent { src_factor: src, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("hud"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("quad"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout { array_stride: 20, step_mode: wgpu::VertexStepMode::Vertex, attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Unorm8x4] }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("ink"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: Some(wgpu::BlendState { color: over(wgpu::BlendFactor::SrcAlpha), alpha: over(wgpu::BlendFactor::One) }), write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let verts = device.create_buffer(&wgpu::BufferDescriptor { label: Some("hud quads"), size: (MAX_QUADS * 4 * 20) as u64, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let list: Vec<u16> = (0..MAX_QUADS as u16).flat_map(|q| [0u16, 1, 2, 0, 2, 3].map(|o| q * 4 + o)).collect();
        let indices = wgpu::util::DeviceExt::create_buffer_init(device, &wgpu::util::BufferInitDescriptor { label: Some("hud indices"), contents: bytemuck::cast_slice(&list), usage: wgpu::BufferUsages::INDEX });
        Ok(Painter { font: Rc::new(font), pipeline, group, screen, verts, indices, quads: 0 })
    }

    /// Hands a frame's batch to the GPU, for a screen of `width × height` pixels.
    pub fn write(&mut self, gpu: &Gpu, hud: &Hud, width: u32, height: u32) {
        gpu.queue.write_buffer(&self.screen, 0, bytemuck::cast_slice(&[width as f32, height as f32, 0.0, 0.0]));
        self.quads = (hud.verts.len() / 4) as u32;
        if self.quads > 0 {
            gpu.queue.write_buffer(&self.verts, 0, bytemuck::cast_slice(&hud.verts));
        }
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass) {
        if self.quads == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.group, &[]);
        pass.set_vertex_buffer(0, self.verts.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(0..self.quads * 6, 0, 0..1);
    }
}
