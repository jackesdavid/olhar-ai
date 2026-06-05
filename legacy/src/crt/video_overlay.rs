//! GPU video compositing for the CRT window.
//!
//! [`NostromoPost`] is the backend's [`PostProcessor`]: it draws the latest
//! camera frame as a real texture into the *text* texture (over the blanked
//! viewport cells, at full pixel resolution), then delegates to the stock
//! [`CrtPostProcessor`]. Because the CRT samples that combined texture through
//! its `curve()` warp, scanlines, aperture mask and ghosting, the video bulges
//! with the glass and lines up exactly with the panel border drawn in the
//! surrounding cells — it sits *in* the CRT rather than floating on top. This
//! also decouples image sharpness from the font/cell size: text stays at a
//! readable 20px while the feed is as crisp as the source allows.
//!
//! `process` only receives a `Queue` (no `Device`), so all GPU objects are built
//! up front in `compile`/`resize`; each frame is uploaded into a fixed max-size
//! texture via `queue.write_texture` and shown through a UV sub-rect. The text
//! texture is `Rgba8Unorm`, so the overlay pipeline targets that format and
//! writes linear values — the CRT applies sRGB/brightness uniformly afterward.
use std::time::Instant;

use ratatui_wgpu::shaders::{CrtPostProcessor, CrtSettings};
use ratatui_wgpu::wgpu::{
    AddressMode, BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingResource, BindingType, BlendComponent, BlendFactor,
    BlendOperation, BlendState, Buffer, BufferBindingType, BufferDescriptor, BufferUsages,
    ColorTargetState, ColorWrites, CommandEncoder, Device, Extent3d, FilterMode, FragmentState,
    LoadOp, MultisampleState, Operations, PipelineCompilationOptions, PipelineLayoutDescriptor,
    PrimitiveState, Queue, RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline,
    RenderPipelineDescriptor, SamplerBindingType, SamplerDescriptor, ShaderModuleDescriptor,
    ShaderSource, ShaderStages, StoreOp, SurfaceConfiguration, Texture, TexelCopyBufferLayout,
    TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType, TextureUsages,
    TextureView, TextureViewDescriptor, TextureViewDimension, VertexState,
};
use ratatui_wgpu::PostProcessor;

use crate::ui::video::Compositor;

/// Fixed texture allocation the frame is uploaded into (camera frames are
/// 960×540; this leaves headroom and avoids reallocating per resize).
const MAX_W: u32 = 1920;
const MAX_H: u32 = 1080;

const SHADER: &str = r#"
struct Q {
  rect: vec4<f32>,   // x0, y0, x1, y1 in NDC (y0 = top)
  uv:   vec4<f32>,   // u_scale, v_scale, _, _
};
@group(0) @binding(0) var<uniform> q: Q;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

struct VsOut {
  @builtin(position) pos: vec4<f32>,
  @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32) -> VsOut {
  var corners = array<vec2<f32>, 6>(
    vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
    vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0));
  let c = corners[vi];
  var o: VsOut;
  o.pos = vec4<f32>(mix(q.rect.x, q.rect.z, c.x), mix(q.rect.y, q.rect.w, c.y), 0.0, 1.0);
  o.uv = vec2<f32>(c.x * q.uv.x, c.y * q.uv.y);
  return o;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
  let l = textureSample(tex, samp, in.uv).r;
  // Phosphor tint to match the octant renderer's green (0, lum, lum/5).
  return vec4<f32>(0.0, l, l * 0.2, 1.0);
}
"#;

/// Owns the pipeline, the upload texture, and the placement uniform for drawing
/// one camera frame as a textured quad.
struct VideoOverlay {
    pipeline: RenderPipeline,
    bind_group: BindGroup,
    uniforms: Buffer,
    texture: Texture,
}

/// The text texture (`text_dest`) handed to the post-processor — we render the
/// camera into it, so the overlay pipeline must target this format.
const TEXT_FORMAT: TextureFormat = TextureFormat::Rgba8Unorm;

impl VideoOverlay {
    fn new(device: &Device) -> Self {
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("camera frame"),
            size: Extent3d { width: MAX_W, height: MAX_H, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::R8Unorm,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&TextureViewDescriptor::default());

        let sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("camera sampler"),
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mipmap_filter: FilterMode::Nearest,
            ..Default::default()
        });

        let uniforms = device.create_buffer(&BufferDescriptor {
            label: Some("camera placement"),
            size: 32, // 2 × vec4<f32>
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bgl = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("camera bgl"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::VERTEX_FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("camera bind group"),
            layout: &bgl,
            entries: &[
                BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() },
                BindGroupEntry { binding: 1, resource: BindingResource::TextureView(&view) },
                BindGroupEntry { binding: 2, resource: BindingResource::Sampler(&sampler) },
            ],
        });

        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("camera shader"),
            source: ShaderSource::Wgsl(SHADER.into()),
        });

        let layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("camera pipeline layout"),
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("camera pipeline"),
            layout: Some(&layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &[],
                compilation_options: PipelineCompilationOptions::default(),
            },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                targets: &[Some(ColorTargetState {
                    format: TEXT_FORMAT,
                    blend: Some(BlendState::REPLACE),
                    write_mask: ColorWrites::ALL,
                })],
                compilation_options: PipelineCompilationOptions::default(),
            }),
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        VideoOverlay { pipeline, bind_group, uniforms, texture }
    }

    /// Upload the current frame and draw it into its cell rectangle within the
    /// text texture (`target`), so the subsequent CRT pass warps and shades it
    /// together with the surrounding text.
    fn draw(&self, queue: &Queue, encoder: &mut CommandEncoder, target: &TextureView, c: &Compositor) {
        let (w, h) = (c.frame_w, c.frame_h);
        if w == 0 || h == 0 || w > MAX_W || h > MAX_H || (c.gray.len() as u32) < w * h {
            return;
        }

        queue.write_texture(
            self.texture.as_image_copy(),
            &c.gray,
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w),
                rows_per_image: Some(h),
            },
            Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );

        // Cell rect → NDC, mapping the grid onto the full surface. y is flipped
        // (NDC top = +1). UV uses only the populated sub-rect of the max texture.
        let (rx, ry, rw, rh) = c.rect;
        let cols = c.cols.max(1) as f32;
        let rows = c.rows.max(1) as f32;
        let x0 = (rx as f32 / cols) * 2.0 - 1.0;
        let x1 = ((rx + rw) as f32 / cols) * 2.0 - 1.0;
        let y0 = 1.0 - (ry as f32 / rows) * 2.0;
        let y1 = 1.0 - ((ry + rh) as f32 / rows) * 2.0;
        let us = w as f32 / MAX_W as f32;
        let vs = h as f32 / MAX_H as f32;

        let mut bytes = [0u8; 32];
        for (i, v) in [x0, y0, x1, y1, us, vs, 0.0, 0.0].iter().enumerate() {
            bytes[i * 4..i * 4 + 4].copy_from_slice(&v.to_ne_bytes());
        }
        queue.write_buffer(&self.uniforms, 0, &bytes);

        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("camera composite"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: Operations { load: LoadOp::Load, store: StoreOp::Store },
                depth_slice: None,
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..6, 0..1);
    }
}

/// The Alien-monitor "refresh roll": a soft, wide brightness wave that drifts
/// slowly from the top of the glass to the bottom and wraps. WGSL only — driven
/// by a single time uniform; tuning lives in [`RollingBand::PARAMS`].
const BAND_SHADER: &str = r#"
struct U { p: vec4<f32> };  // x: time(s), y: speed(passes/s), z: half-width, w: intensity
@group(0) @binding(0) var<uniform> u: U;

struct VsOut {
  @builtin(position) pos: vec4<f32>,
  @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32) -> VsOut {
  // A single oversized triangle covering the screen.
  let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
  var o: VsOut;
  o.pos = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
  o.uv = uv;
  return o;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
  let sy = 1.0 - in.uv.y;                 // 0 at top of screen .. 1 at bottom
  let center = fract(u.p.x * u.p.y);      // sweeps top->bottom, wraps seamlessly
  var d = abs(sy - center);
  d = min(d, 1.0 - d);                    // wrap distance so there is no seam
  let band = smoothstep(u.p.z, 0.0, d) * u.p.w;
  // Soft green phosphor glow, premultiplied so it composites additively.
  let glow = band * vec3<f32>(0.16, 1.0, 0.32);
  return vec4<f32>(glow, band);
}
"#;

/// Draws the refresh roll as a single additive full-screen pass over the CRT
/// output. Self-contained: own pipeline + a 16-byte time/param uniform.
struct RollingBand {
    pipeline: RenderPipeline,
    bind_group: BindGroup,
    uniforms: Buffer,
    start: Instant,
}

impl RollingBand {
    /// Fixed tuning: passes-per-second (≈ one drift every 5.5s) and the band's
    /// soft half-height (fraction of the screen). Kept deliberately gentle — a
    /// slow wave, not a strobing bar.
    const PARAMS: (f32, f32) = (0.18, 0.22);

    /// Peak brightness at full user strength. The live `sweep_intensity` (0..1)
    /// scales this; the default strength of 0.5 lands at a gentle ~0.045.
    const MAX_INTENSITY: f32 = 0.09;

    fn new(device: &Device, format: TextureFormat) -> Self {
        let uniforms = device.create_buffer(&BufferDescriptor {
            label: Some("refresh-roll uniforms"),
            size: 16, // one vec4<f32>
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bgl = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("refresh-roll bgl"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("refresh-roll bind group"),
            layout: &bgl,
            entries: &[BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() }],
        });

        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("refresh-roll shader"),
            source: ShaderSource::Wgsl(BAND_SHADER.into()),
        });

        let layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("refresh-roll pipeline layout"),
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });

        // Additive over the existing CRT output: source already premultiplied.
        let blend = BlendState {
            color: BlendComponent {
                src_factor: BlendFactor::One,
                dst_factor: BlendFactor::One,
                operation: BlendOperation::Add,
            },
            alpha: BlendComponent {
                src_factor: BlendFactor::One,
                dst_factor: BlendFactor::One,
                operation: BlendOperation::Add,
            },
        };

        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("refresh-roll pipeline"),
            layout: Some(&layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &[],
                compilation_options: PipelineCompilationOptions::default(),
            },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                targets: &[Some(ColorTargetState {
                    format,
                    blend: Some(blend),
                    write_mask: ColorWrites::ALL,
                })],
                compilation_options: PipelineCompilationOptions::default(),
            }),
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        RollingBand { pipeline, bind_group, uniforms, start: Instant::now() }
    }

    /// Paint the band onto `target` (the swapchain view) on top of whatever the
    /// CRT pass already wrote (`LoadOp::Load`).
    fn draw(&self, queue: &Queue, encoder: &mut CommandEncoder, target: &TextureView) {
        let (speed, width) = Self::PARAMS;
        let intensity = crate::ui::video::sweep_intensity() * Self::MAX_INTENSITY;
        let secs = self.start.elapsed().as_secs_f32();
        let mut bytes = [0u8; 16];
        for (i, v) in [secs, speed, width, intensity].iter().enumerate() {
            bytes[i * 4..i * 4 + 4].copy_from_slice(&v.to_ne_bytes());
        }
        queue.write_buffer(&self.uniforms, 0, &bytes);

        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("refresh-roll pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: Operations { load: LoadOp::Load, store: StoreOp::Store },
                depth_slice: None,
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// User data handed to the backend at build time: the CRT settings plus the
/// shared compositor the GPU side reads each frame.
pub struct NostromoData {
    pub crt: CrtSettings,
    pub state: &'static std::sync::Mutex<Compositor>,
}

/// The composed post-processor: stock CRT look, the camera texture composited
/// into it, then the refresh-roll sweep painted over the final image.
pub struct NostromoPost {
    crt: CrtPostProcessor,
    video: VideoOverlay,
    band: RollingBand,
    state: &'static std::sync::Mutex<Compositor>,
}

impl PostProcessor for NostromoPost {
    type UserData = NostromoData;

    fn compile(
        device: &Device,
        text_view: &TextureView,
        surface_config: &SurfaceConfiguration,
        user_data: Self::UserData,
    ) -> Self {
        let crt = CrtPostProcessor::compile(device, text_view, surface_config, user_data.crt);
        let video = VideoOverlay::new(device);
        let band = RollingBand::new(device, surface_config.format);
        NostromoPost { crt, video, band, state: user_data.state }
    }

    fn resize(&mut self, device: &Device, text_view: &TextureView, surface_config: &SurfaceConfiguration) {
        // The overlay is resolution-independent (NDC from grid fractions, fixed
        // texture); only the CRT processor depends on the surface size.
        self.crt.resize(device, text_view, surface_config);
    }

    fn process(
        &mut self,
        encoder: &mut CommandEncoder,
        queue: &Queue,
        text_view: &TextureView,
        surface_config: &SurfaceConfiguration,
        surface_view: &TextureView,
    ) {
        // Composite the camera into the text texture FIRST, then let the CRT
        // warp + shade text and video together so the image sits in the glass.
        if let Ok(c) = self.state.lock() {
            if c.active {
                self.video.draw(queue, encoder, text_view, &c);
            }
        }
        self.crt.process(encoder, queue, text_view, surface_config, surface_view);

        // Finally, drift the refresh roll over the finished CRT image.
        if crate::ui::video::sweep_enabled() {
            self.band.draw(queue, encoder, surface_view);
        }
    }

    fn needs_update(&self) -> bool {
        // Keep repainting while a feed is on screen (so the video animates) or
        // while the refresh roll is on (so the sweep keeps drifting) even when
        // the surrounding text is static.
        crate::ui::video::sweep_enabled() || self.state.lock().map(|c| c.active).unwrap_or(false)
    }
}
