use egui_wgpu::renderer::ScreenDescriptor;
use wgpu::*;
use winit::{dpi::PhysicalSize, window::Window};

pub struct GpuState {
    pub device: Device,
    pub queue: Queue,
    pub surface: Surface,
    pub surface_config: SurfaceConfiguration,
    pub sample_count: u32,
    pub msaa_texture_view: Option<TextureView>,
}
impl GpuState {
    pub async fn new(window: &Window) -> Self {
        let instance = Instance::new(InstanceDescriptor {
            backends: Backends::all(),
            dx12_shader_compiler: Default::default(),
        });

        let surface = unsafe { instance.create_surface(window) }.unwrap();

        let request_adapter_options = RequestAdapterOptionsBase {
            power_preference: PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
        };
        let adapter = instance
            .request_adapter(&request_adapter_options)
            .await
            .unwrap();

        let device_descriptor = DeviceDescriptor {
            label: None,
            features: Features::empty() | Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
            limits: Limits::default(),
        };
        let (device, queue) = adapter
            .request_device(&device_descriptor, None)
            .await
            .unwrap();

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);
        let mut sample_count = 16;
        while !adapter
            .get_texture_format_features(surface_format)
            .flags
            .sample_count_supported(sample_count)
        {
            sample_count /= 2;
        }
        let size = window.inner_size();
        let surface_config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: surface_caps.present_modes[0],
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &surface_config);
        let mut out = Self {
            device,
            queue,
            surface,
            surface_config,
            msaa_texture_view: None,
            sample_count,
        };
        if sample_count > 1 {
            out.msaa_texture_view = Some(out.create_msaa_texture_view());
        }
        out
    }
    fn create_msaa_texture_view(&self) -> TextureView {
        self.device
            .create_texture(&TextureDescriptor {
                label: Some("Multisample Framebuffer"),
                size: Extent3d {
                    width: self.surface_config.width,
                    height: self.surface_config.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: self.sample_count,
                dimension: TextureDimension::D2,
                format: self.surface_config.format,
                usage: TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&TextureViewDescriptor::default())
    }
    pub fn resize(&mut self, size: &PhysicalSize<u32>) {
        if size.height * size.width == 0 {
            return;
        }
        self.surface_config.width = size.width;
        self.surface_config.height = size.height;
        self.surface.configure(&self.device, &self.surface_config);
        if self.sample_count > 1 {
            self.msaa_texture_view = Some(self.create_msaa_texture_view());
        }
    }
    pub fn get_screen_descriptor(&self, window: &Window) -> ScreenDescriptor {
        ScreenDescriptor {
            size_in_pixels: [self.surface_config.width, self.surface_config.height],
            pixels_per_point: window.scale_factor() as f32,
        }
    }
    pub fn create_render_pass<'a>(
        &'a self,
        encoder: &'a mut CommandEncoder,
        output_texture: &'a TextureView,
    ) -> RenderPass {
        let mut color_attachment = RenderPassColorAttachment {
            view: output_texture,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(Color::BLACK),
                store: true,
            },
        };
        if self.sample_count > 1 {
            color_attachment.view = self.msaa_texture_view.as_ref().unwrap();
            color_attachment.resolve_target = Some(output_texture);
            color_attachment.ops.store = false;
        }
        let render_pass_descriptor = RenderPassDescriptor {
            label: Some("Render Pass"),
            depth_stencil_attachment: None,
            color_attachments: &mut [Some(color_attachment)],
        };
        encoder.begin_render_pass(&render_pass_descriptor)
    }
    pub fn create_command_encoder(&self) -> CommandEncoder {
        self.device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            })
    }
    pub fn get_surface_texture(&self) -> Result<(SurfaceTexture, TextureView), SurfaceError> {
        let surface_texture = self.surface.get_current_texture()?;
        let texture_view = surface_texture
            .texture
            .create_view(&TextureViewDescriptor::default());
        Ok((surface_texture, texture_view))
    }
}
