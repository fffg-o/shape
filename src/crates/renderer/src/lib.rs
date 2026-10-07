use std::borrow::Cow;
use wgpu::{
    BindGroup,
    BindGroupLayout,
    Device,
    Extent3d,
    FragmentState,
    Instance,
    Queue,
    RenderPipeline,
    RenderPipelineDescriptor,
    Sampler,
    ShaderModuleDescriptor,
    ShaderSource,
    Surface,
    SurfaceConfiguration,
    Texture,
    TextureView,
    TextureViewDescriptor,
    VertexState,
};
use winit::window::Window;

pub struct Renderer<'a> {
    pub surface: Surface<'a>,
    pub device: Device,
    pub queue: Queue,
    pub config: SurfaceConfiguration,
    pipeline: RenderPipeline,
    texture: Option<Texture>,
    texture_view: Option<TextureView>,
    sampler: Sampler,
    bind_group_layout: BindGroupLayout,
    bind_group: Option<BindGroup>,
}

impl<'a> Renderer<'a> {
    pub async fn new(window: &'a Window) -> Self {
        let instance = Instance::default();

        let surface = instance
            .create_surface(window)
            .unwrap();

        let adapter = instance
            .request_adapter(
                &wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: Some(&surface),
                    force_fallback_adapter: false,
                },
            )
            .await
            .unwrap();

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("Remote Desktop Device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    experimental_features: Default::default(),
                    memory_hints: wgpu::MemoryHints::Performance,
                    trace: wgpu::Trace::Off,
                },
            )
            .await
            .unwrap();

        let size = window.inner_size();

        let capabilities = surface.get_capabilities(&adapter);

        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(|format| format.is_srgb())
            .unwrap_or(capabilities.formats[0]);

        let present_mode = capabilities
            .present_modes
            .iter()
            .copied()
            .find(|mode| *mode == wgpu::PresentMode::Fifo)
            .unwrap_or(capabilities.present_modes[0]);

        let config = SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode,
            alpha_mode: capabilities.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&device, &config);

        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("Desktop Shader"),
            source: ShaderSource::Wgsl(
                Cow::from(r#"
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(
    @builtin(vertex_index) index: u32
) -> VertexOutput {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0)
    );

    var uvs = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 1.0),
        vec2<f32>(2.0, 1.0),
        vec2<f32>(0.0, -1.0)
    );

    var output: VertexOutput;
    output.position = vec4<f32>(positions[index], 0.0, 1.0);
    output.uv = uvs[index];
    return output;
}

@group(0) @binding(0)
var desktop_texture: texture_2d<f32>;

@group(0) @binding(1)
var desktop_sampler: sampler;

@fragment
fn fs_main(
    input: VertexOutput
) -> @location(0) vec4<f32> {
    return textureSample(
        desktop_texture,
        desktop_sampler,
        input.uv
    );
}
"#),
            ),
        });

        let bind_group_layout =
            device.create_bind_group_layout(
                &wgpu::BindGroupLayoutDescriptor {
                    label: Some("Desktop Bind Group Layout"),
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Texture {
                                sample_type: wgpu::TextureSampleType::Float {
                                    filterable: true,
                                },
                                view_dimension: wgpu::TextureViewDimension::D2,
                                multisampled: false,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 1,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Sampler(
                                wgpu::SamplerBindingType::Filtering,
                            ),
                            count: None,
                        },
                    ],
                },
            );

        let pipeline_layout =
            device.create_pipeline_layout(
                &wgpu::PipelineLayoutDescriptor {
                    label: Some("Desktop Pipeline Layout"),
                    bind_group_layouts: &[
                        &bind_group_layout,
                    ],
                    push_constant_ranges: &[],
                },
            );

        let pipeline = device.create_render_pipeline(
            &RenderPipelineDescriptor {
                label: Some("Desktop Pipeline"),
                layout: Some(&pipeline_layout),
                vertex: VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options:
                    wgpu::PipelineCompilationOptions::default(),
                },
                primitive:
                wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample:
                wgpu::MultisampleState::default(),
                fragment: Some(FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(
                        wgpu::ColorTargetState {
                            format,
                            blend: None,
                            write_mask:
                            wgpu::ColorWrites::ALL,
                        },
                    )],
                    compilation_options:
                    wgpu::PipelineCompilationOptions::default(),
                }),
                cache: None,
                multiview: None,
            },
        );

        let sampler = device.create_sampler(
            &wgpu::SamplerDescriptor {
                label: Some("Desktop Sampler"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::FilterMode::Nearest,
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                ..Default::default()
            },
        );

        Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            texture: None,
            texture_view: None,
            sampler,
            bind_group_layout,
            bind_group: None,
        }
    }

    pub fn update_frame(
        &mut self,
        width: u32,
        height: u32,
        data: &[u8],
    ) {
        if width == 0 || height == 0 {
            return;
        }

        let required_len =
            width as usize * height as usize * 4;

        if data.len() < required_len {
            return;
        }

        let needs_new_texture = match &self.texture {
            Some(texture) => {
                let size = texture.size();

                size.width != width
                    || size.height != height
            }
            None => true,
        };

        if needs_new_texture {
            let texture = self.device.create_texture(
                &wgpu::TextureDescriptor {
                    label: Some("Desktop Texture"),
                    size: Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension:
                    wgpu::TextureDimension::D2,
                    format:
                    wgpu::TextureFormat::Bgra8Unorm,
                    usage:
                    wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                },
            );

            let texture_view = texture.create_view(
                &TextureViewDescriptor::default(),
            );

            let bind_group =
                self.device.create_bind_group(
                    &wgpu::BindGroupDescriptor {
                        label: Some("Desktop Bind Group"),
                        layout:
                        &self.bind_group_layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource:
                                wgpu::BindingResource::TextureView(
                                    &texture_view,
                                ),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource:
                                wgpu::BindingResource::Sampler(
                                    &self.sampler,
                                ),
                            },
                        ],
                    },
                );

            self.texture = Some(texture);
            self.texture_view = Some(texture_view);
            self.bind_group = Some(bind_group);
        }

        if let Some(texture) = &self.texture {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &data[..required_len],
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width * 4),
                    rows_per_image: Some(height),
                },
                Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
        }
    }

    pub fn resize(
        &mut self,
        width: u32,
        height: u32,
    ) {
        if width == 0 || height == 0 {
            return;
        }

        self.config.width = width;
        self.config.height = height;

        self.surface.configure(
            &self.device,
            &self.config,
        );
    }

    pub fn render(&self) {
        let output =
            match self.surface.get_current_texture() {
                Ok(output) => output,
                Err(wgpu::SurfaceError::Lost) => {
                    self.surface.configure(
                        &self.device,
                        &self.config,
                    );
                    return;
                }
                Err(
                    wgpu::SurfaceError::OutOfMemory,
                ) => {
                    return;
                }
                Err(_) => {
                    return;
                }
            };

        let view = output
            .texture
            .create_view(
                &TextureViewDescriptor::default(),
            );

        let mut encoder =
            self.device.create_command_encoder(
                &wgpu::CommandEncoderDescriptor {
                    label: Some("Render Encoder"),
                },
            );

        {
            let mut render_pass =
                encoder.begin_render_pass(
                    &wgpu::RenderPassDescriptor {
                        label: Some("Render Pass"),
                        color_attachments: &[Some(
                            wgpu::RenderPassColorAttachment {
                                view: &view,
                                depth_slice: None,
                                resolve_target: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(
                                        wgpu::Color {
                                            r: 0.05,
                                            g: 0.05,
                                            b: 0.05,
                                            a: 1.0,
                                        },
                                    ),
                                    store:
                                    wgpu::StoreOp::Store,
                                },
                            },
                        )],
                        depth_stencil_attachment: None,
                        occlusion_query_set:
                        None,
                        timestamp_writes: None,
                    },
                );

            if let Some(bind_group) =
                &self.bind_group
            {
                render_pass.set_pipeline(
                    &self.pipeline,
                );

                render_pass.set_bind_group(
                    0,
                    bind_group,
                    &[],
                );

                render_pass.draw(
                    0..3,
                    0..1,
                );
            }
        }

        self.queue.submit(
            Some(encoder.finish()),
        );

        output.present();
    }
}