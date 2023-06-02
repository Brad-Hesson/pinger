use wgpu::*;
use winit::{event::Event, window::Window};

pub struct DeviceState {
    pub surface: wgpu::Surface,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    pub size: winit::dpi::PhysicalSize<u32>,
    pub window: Window,
    pub sample_count: u32,
    pub multisample_framebuffer: TextureView,
}

impl DeviceState {
    pub async fn new(window: Window) -> Self {
        let size = window.inner_size();

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            dx12_shader_compiler: Default::default(),
        });

        let surface = unsafe { instance.create_surface(&window) }.unwrap();

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptionsBase {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .await
            .unwrap();

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: None,
                    features: wgpu::Features::empty()
                        | Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
                    limits: wgpu::Limits::default(),
                },
                None,
            )
            .await
            .unwrap();

        let surface_caps = surface.get_capabilities(&adapter);
        tracing::info!("{surface_caps:?}");
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
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: surface_caps.present_modes[0],
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        let multisample_framebuffer = device
            .create_texture(&TextureDescriptor {
                label: Some("Multisample Framebuffer"),
                size: Extent3d {
                    width: size.width,
                    height: size.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count,
                dimension: TextureDimension::D2,
                format: surface_format,
                usage: TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&TextureViewDescriptor::default());

        Self {
            surface,
            device,
            queue,
            config,
            size,
            window,
            sample_count,
            multisample_framebuffer,
        }
    }

    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.size = new_size;
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.surface.configure(&self.device, &self.config);
            self.multisample_framebuffer = self
                .device
                .create_texture(&TextureDescriptor {
                    label: Some("Multisample Framebuffer"),
                    size: Extent3d {
                        width: new_size.width,
                        height: new_size.height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: self.sample_count,
                    dimension: TextureDimension::D2,
                    format: self.config.format,
                    usage: TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                })
                .create_view(&TextureViewDescriptor::default());
        }
    }
    pub fn update(&mut self, event: &Event<()>) {
        use winit::event::WindowEvent::*;
        match event {
            Event::WindowEvent { ref event, .. } => match event {
                ScaleFactorChanged { new_inner_size, .. } => self.resize(**new_inner_size),
                Resized(physical_size) => self.resize(*physical_size),
                _ => {}
            },
            Event::MainEventsCleared => {
                self.window.request_redraw();
            }
            _ => {}
        }
    }
}
