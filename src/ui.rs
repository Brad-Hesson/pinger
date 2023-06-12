use std::path::{Path, PathBuf};

use egui::Context;
use egui_winit::EventResponse;
use wgpu::*;
use winit::{event::WindowEvent, event_loop::ControlFlow};

use crate::gpu::GpuState;

const INITIAL_WIDTH: u32 = 1920;
const INITIAL_HEIGHT: u32 = 1080;

pub async fn main() {
    let event_loop = winit::event_loop::EventLoop::new();
    let window = winit::window::WindowBuilder::new()
        .with_title("Pinger")
        .with_inner_size(winit::dpi::PhysicalSize {
            width: INITIAL_WIDTH,
            height: INITIAL_HEIGHT,
        })
        .build(&event_loop)
        .unwrap();

    let mut gpu = GpuState::new(&window).await;

    let mut egui_platform = egui_winit::State::new(&window);
    egui_platform.set_pixels_per_point(window.scale_factor() as f32);
    let mut egui_renderer = egui_wgpu::Renderer::new(
        &gpu.device,
        gpu.surface_config.format,
        None,
        gpu.msaa.sample_count,
    );
    let egui_ctx = egui::Context::default();

    let mut ui_state = UiState::new();

    event_loop.run(move |event, _, control_flow| match event {
        winit::event::Event::WindowEvent { event, .. } => {
            let EventResponse { consumed, repaint } = egui_platform.on_event(&egui_ctx, &event);
            if repaint {
                window.request_redraw();
            }
            if consumed {
                return;
            }
            match event {
                WindowEvent::Resized(size) => gpu.resize(&size),
                WindowEvent::CloseRequested => {
                    *control_flow = ControlFlow::Exit;
                }
                _ => {}
            };
        }
        winit::event::Event::RedrawRequested(..) => {
            let output_texture = match gpu.surface.get_current_texture() {
                Ok(frame) => frame,
                _ => return,
            };
            let view = output_texture
                .texture
                .create_view(&TextureViewDescriptor::default());

            let input = egui_platform.take_egui_input(&window);
            egui_ctx.begin_frame(input);
            ui_state.run(&egui_ctx);
            let full_output = egui_ctx.end_frame();
            egui_platform.handle_platform_output(&window, &egui_ctx, full_output.platform_output);

            let paint_jobs = egui_ctx.tessellate(full_output.shapes);
            let mut encoder = gpu.create_command_encoder();
            let screen_descriptor = gpu.get_screen_descriptor(&window);
            egui_renderer.update_buffers(
                &gpu.device,
                &gpu.queue,
                &mut encoder,
                &paint_jobs[..],
                &screen_descriptor,
            );
            for (texture_id, image_delta) in full_output.textures_delta.set {
                egui_renderer.update_texture(&gpu.device, &gpu.queue, texture_id, &image_delta);
            }
            egui_renderer.render(
                &mut gpu.create_render_pass(&mut encoder, &view),
                &paint_jobs[..],
                &screen_descriptor,
            );
            gpu.queue.submit(Some(encoder.finish()));
            output_texture.present();
            for texture_id in full_output.textures_delta.free {
                egui_renderer.free_texture(&texture_id);
            }
        }
        winit::event::Event::MainEventsCleared => {
            window.request_redraw();
        }
        _ => {}
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
