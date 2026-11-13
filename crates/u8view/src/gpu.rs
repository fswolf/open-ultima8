//! The wgpu presenter: the world's frame drawn to fill the window at any zoom — crisp texels
//! with smoothed edges, plain sampling, xBR edge smoothing or a CRT's scanlines, all worked out
//! per screen pixel on the GPU (`shader.wgsl`) — and the windows and menus over it at their own
//! scale.
use anyhow::{anyhow, Result};
use std::sync::Arc;
use winit::{event_loop::ActiveEventLoop, window::Window};

/// The uniform block's size (see `U` in the shader).
const UNIFORMS: usize = 96;

const SHADER: &str = include_str!("shader.wgsl");

/// How a layer's texels become screen pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    /// Bilinear (soft).
    Plain = 0,
    /// Crisp texels whatever the zoom; only their edges blend (sharp bilinear).
    Sharp = 1,
    /// Edges found and smoothed per screen pixel (xBR), at any zoom.
    Xbr = 2,
    /// Scanlines and a shadow mask, as on a CRT.
    Crt = 3,
}

/// Where a layer's texture goes on the screen.
#[derive(Clone, Copy, Debug)]
pub struct Place {
    /// Texels per screen pixel, and the texel at the screen's top-left.
    pub k: f32,
    pub off: (f32, f32),
    pub filter: Filter,
}

/// The world, and the windows and menus over it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    World = 0,
    Overlay = 1,
}

struct Tex {
    tex: wgpu::Texture,
    w: u32,
    h: u32,
    bind: wgpu::BindGroup,
}

pub struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    /// Bilinear (the filters that want hard texels make them themselves).
    sampler: wgpu::Sampler,
    /// Per layer: its uniforms and texture.
    ubufs: [wgpu::Buffer; 2],
    texs: [Option<Tex>; 2],
}

impl Gpu {
    pub async fn new(el: &ActiveEventLoop, win: Arc<Window>) -> Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(
            el.owned_display_handle(),
        )));
        let surface = instance.create_surface(win.clone())?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions { compatible_surface: Some(&surface), ..Default::default() })
            .await?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_limits: wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
                ..Default::default()
            })
            .await?;
        let sz = win.inner_size();
        let mut config = surface
            .get_default_config(&adapter, sz.width.max(1), sz.height.max(1))
            .ok_or_else(|| anyhow!("surface unsupported"))?;
        // Prefer a plain 8-bit format so palette bytes reach the screen untouched. HDR/float formats
        // are linear, which made everything look washed out; the texture format below compensates
        // when we can't avoid one.
        let caps = surface.get_capabilities(&adapter);
        use wgpu::TextureFormat as F;
        config.format = [F::Bgra8Unorm, F::Rgba8Unorm, F::Bgra8UnormSrgb, F::Rgba8UnormSrgb]
            .into_iter()
            .find(|f| caps.formats.contains(f))
            .unwrap_or(caps.formats[0]);
        let info = adapter.get_info();
        eprintln!("gpu: {} ({:?}), surface {:?} (offered {:?})", info.name, info.backend, config.format, caps.formats);
        config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&device, &config);

        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty,
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                entry(
                    0,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                entry(1, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
                entry(
                    2,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let ubuf = || {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: UNIFORMS as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let ubufs = [ubuf(), ubuf()];
        Ok(Self { surface, device, queue, config, pipeline, layout, sampler, ubufs, texs: [None, None] })
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        if w == 0 || h == 0 {
            return;
        }
        self.config.width = w;
        self.config.height = h;
        self.surface.configure(&self.device, &self.config);
    }

    /// The layer's pixels (premultiplied RGBA, `w`×`h`).
    pub fn upload(&mut self, layer: Layer, rgba: &[u8], w: u32, h: u32) {
        let max = self.device.limits().max_texture_dimension_2d;
        if w == 0 || h == 0 || w > max || h > max || rgba.len() != (w * h * 4) as usize {
            eprintln!("skipping upload: {w}x{h} (max {max}, {} bytes)", rgba.len());
            return;
        }
        let n = layer as usize;
        if self.texs[n].as_ref().is_none_or(|t| (t.w, t.h) != (w, h)) {
            let size = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
            let tex = self.device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                // Match the surface's encoding so palette colours pass through unchanged.
                format: if self.config.format.is_srgb() || self.config.format == wgpu::TextureFormat::Rgba16Float {
                    wgpu::TextureFormat::Rgba8UnormSrgb
                } else {
                    wgpu::TextureFormat::Rgba8Unorm
                },
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = tex.create_view(&Default::default());
            let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                    wgpu::BindGroupEntry { binding: 2, resource: self.ubufs[n].as_entire_binding() },
                ],
            });
            self.texs[n] = Some(Tex { tex, w, h, bind });
        }
        let Some(t) = &self.texs[n] else { return };
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &t.tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
    }

    /// Show the world with the overlay over it, each placed as given (None: not shown). `gamma` below 1 brightens,
    /// above 1 darkens; `linearize` undoes an unwanted sRGB encode by the display; `fade` is
    /// the game's colour matrix (rows of r, g, b weights and a constant); `inverted` how far the
    /// screen has turned upside down (0 upright, 0x8000 over: see `u8engine::fade::Inverter`).
    pub fn present(&mut self, places: [Option<Place>; 2], gamma: f32, linearize: bool, fade: [f32; 12], inverted: u16) {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            _ => return,
        };
        for (n, p) in places.iter().enumerate() {
            let (Some(t), Some(p)) = (&self.texs[n], p) else { continue };
            let head =
                [p.k, p.filter as u8 as f32, gamma, linearize as u8 as f32, p.off.0, p.off.1, t.w as f32, t.h as f32];
            let mut u = [0u8; UNIFORMS];
            let tail = [inverted as f32, self.config.height as f32, 0.0, 0.0];
            for (k, v) in head.iter().chain(fade.iter()).chain(tail.iter()).enumerate() {
                u[k * 4..k * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
            self.queue.write_buffer(&self.ubufs[n], 0, &u);
        }
        let view = frame.texture.create_view(&Default::default());
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            rp.set_pipeline(&self.pipeline);
            for (t, _) in self.texs.iter().zip(places).filter_map(|(t, p)| t.as_ref().zip(p)) {
                rp.set_bind_group(0, &t.bind, &[]);
                rp.draw(0..3, 0..1);
            }
        }
        self.queue.submit([enc.finish()]);
        self.queue.present(frame);
    }
}

#[cfg(test)]
mod tests {
    /// The shader parses and validates (wgpu would only say so when the window opens).
    #[test]
    fn the_shader_compiles() {
        let module = naga::front::wgsl::parse_str(super::SHADER)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(super::SHADER)));
        let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
            .validate(&module)
            .unwrap_or_else(|e| panic!("{e:?}"));
        // (Vulkan's form of it, as Linux drivers get it.)
        naga::back::spv::write_vec(&module, &info, &Default::default(), None).expect("SPIR-V");
    }
}
