use std::path::{Path, PathBuf};

use egui::{Align, Context, FullOutput};
use egui_wgpu::renderer::ScreenDescriptor;
use egui_winit::EventResponse;
use wgpu::*;
use winit::{event::WindowEvent, event_loop::ControlFlow};

const INITIAL_WIDTH: u32 = 1920;
const INITIAL_HEIGHT: u32 = 1080;

pub async fn main() {
    let event_loop = winit::event_loop::EventLoop::new();
    let window = winit::window::WindowBuilder::new()
        .with_decorations(true)
        .with_resizable(true)
        .with_transparent(false)
        .with_title("Pinger")
        .with_inner_size(winit::dpi::PhysicalSize {
            width: INITIAL_WIDTH,
            height: INITIAL_HEIGHT,
        })
        .build(&event_loop)
        .unwrap();

    let instance = Instance::new(InstanceDescriptor {
        backends: Backends::all(),
        dx12_shader_compiler: Default::default(),
    });

    let surface = unsafe { instance.create_surface(&window) }.unwrap();

    let adapter = instance
        .request_adapter(&RequestAdapterOptionsBase {
            power_preference: PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
        })
        .await
        .unwrap();

    let (device, queue) = adapter
        .request_device(
            &DeviceDescriptor {
                label: None,
                features: Features::empty() | Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
                limits: Limits::default(),
            },
            None,
        )
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
    let mut msaa_texture_view = device
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
    let mut surface_config = SurfaceConfiguration {
        usage: TextureUsages::RENDER_ATTACHMENT,
        format: surface_format,
        width: size.width,
        height: size.height,
        present_mode: surface_caps.present_modes[0],
        alpha_mode: surface_caps.alpha_modes[0],
        view_formats: vec![],
    };
    surface.configure(&device, &surface_config);

    let mut egui_platform = egui_winit::State::new(&window);
    egui_platform.set_pixels_per_point(window.scale_factor() as f32);
    let mut ui_state = UiState::new();

    let mut egui_renderer = egui_wgpu::Renderer::new(&device, surface_format, None, sample_count);
    let egui_ctx = egui::Context::default();

    event_loop.run(move |event, _, control_flow| {
        if let winit::event::Event::WindowEvent { event, .. } = &event {
            let EventResponse { consumed, repaint } = egui_platform.on_event(&egui_ctx, event);
            if repaint {
                window.request_redraw();
            }
            if consumed {
                return;
            }
        }

        match event {
            winit::event::Event::RedrawRequested(..) => {
                let output_texture = match surface.get_current_texture() {
                    Ok(frame) => frame,
                    _ => return,
                };
                let output_texture_view = output_texture
                    .texture
                    .create_view(&TextureViewDescriptor::default());
                let input = egui_platform.take_egui_input(&window);
                egui_ctx.begin_frame(input);
                ui_state.run(&egui_ctx);
                let FullOutput {
                    platform_output,
                    textures_delta,
                    shapes,
                    repaint_after: _,
                } = egui_ctx.end_frame();
                egui_platform.handle_platform_output(&window, &egui_ctx, platform_output);
                let paint_jobs = egui_ctx.tessellate(shapes);
                let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
                    label: Some("Render Encoder"),
                });
                let screen_descriptor = ScreenDescriptor {
                    size_in_pixels: [surface_config.width, surface_config.height],
                    pixels_per_point: window.scale_factor() as f32,
                };
                egui_renderer.update_buffers(
                    &device,
                    &queue,
                    &mut encoder,
                    &paint_jobs[..],
                    &screen_descriptor,
                );
                for (texture_id, image_delta) in textures_delta.set {
                    egui_renderer.update_texture(&device, &queue, texture_id, &image_delta);
                }
                {
                    let mut color_attachment = RenderPassColorAttachment {
                        view: &output_texture_view,
                        resolve_target: None,
                        ops: Operations {
                            load: LoadOp::Clear(Color::BLACK),
                            store: true,
                        },
                    };
                    if sample_count > 1 {
                        color_attachment.view = &msaa_texture_view;
                        color_attachment.resolve_target = Some(&output_texture_view);
                        color_attachment.ops.store = false;
                    }
                    let render_pass_descriptor = RenderPassDescriptor {
                        label: Some("Render Pass"),
                        depth_stencil_attachment: None,
                        color_attachments: &mut [Some(color_attachment)],
                    };
                    let mut render_pass = encoder.begin_render_pass(&render_pass_descriptor);
                    egui_renderer.render(&mut render_pass, &paint_jobs[..], &screen_descriptor);
                }
                queue.submit(Some(encoder.finish()));
                output_texture.present();
                for texture_id in textures_delta.free {
                    egui_renderer.free_texture(&texture_id);
                }
            }
            winit::event::Event::MainEventsCleared => {
                window.request_redraw();
            }
            winit::event::Event::WindowEvent { event, .. } => match event {
                WindowEvent::Resized(size) => {
                    if size.height * size.width != 0 {
                        surface_config.width = size.width;
                        surface_config.height = size.height;
                        surface.configure(&device, &surface_config);
                        msaa_texture_view = device
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
                    }
                }
                WindowEvent::CloseRequested => {
                    *control_flow = ControlFlow::Exit;
                }
                _ => {}
            },
            _ => {}
        }
    })
}

struct UiState {
    file_dialog: Option<egui_file::FileDialog>,
    current_file: Option<PathBuf>,
}
impl UiState {
    fn new() -> Self {
        Self {
            file_dialog: None,
            current_file: None,
        }
    }
    fn run(&mut self, ctx: &Context) {
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open").clicked() {
                        let mut file_dialog = egui_file::FileDialog::open_file(None).filter(
                            Box::new(|path: &Path| path.extension().is_some_and(|s| s == "ping")),
                        );
                        file_dialog.open();
                        self.file_dialog = Some(file_dialog);
                    }
                });
                ui.label(format!("Current File: {:?}", self.current_file));
            })
        });
        if let Some(ref mut file_dialog) = self.file_dialog {
            if file_dialog.show(ctx).selected() {
                self.current_file = Some(file_dialog.path().unwrap())
            };
        }
    }
}
