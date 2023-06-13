use std::{
    iter,
    path::{Path, PathBuf},
    sync::Arc,
};

use egui::Context;
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
        gpu.sample_count,
    );
    let egui_ctx = egui::Context::default();

    let mut ui_state = UiState::new();

    event_loop.run(move |event, _, control_flow| match event {
        winit::event::Event::WindowEvent { event, .. } => {
            let egui_result = egui_platform.on_event(&egui_ctx, &event);
            if egui_result.repaint {
                window.request_redraw();
            }
            if egui_result.consumed {
                return;
            }
            match event {
                WindowEvent::Resized(size) => {
                    gpu.resize(&size);
                    window.request_redraw()
                }
                WindowEvent::ScaleFactorChanged { new_inner_size, .. } => {
                    gpu.resize(&new_inner_size);
                    window.request_redraw()
                }
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

            let egui_input = egui_platform.take_egui_input(&window);
            egui_ctx.begin_frame(egui_input);
            ui_state.run(&egui_ctx);
            let egui_output = egui_ctx.end_frame();
            egui_platform.handle_platform_output(&window, &egui_ctx, egui_output.platform_output);

            let mut encoder = gpu.create_command_encoder();
            let egui_primitives = egui_ctx.tessellate(egui_output.shapes);
            let screen_descriptor = gpu.get_screen_descriptor(&window);
            egui_renderer.update_buffers(
                &gpu.device,
                &gpu.queue,
                &mut encoder,
                &egui_primitives[..],
                &screen_descriptor,
            );
            for (texture_id, image_delta) in egui_output.textures_delta.set {
                egui_renderer.update_texture(&gpu.device, &gpu.queue, texture_id, &image_delta);
            }
            egui_renderer.render(
                &mut gpu.create_render_pass(&mut encoder, &view),
                &egui_primitives[..],
                &screen_descriptor,
            );
            gpu.queue.submit(iter::once(encoder.finish()));
            output_texture.present();
            for texture_id in egui_output.textures_delta.free {
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
    file_open_dialog: egui_file::FileDialog,
    current_file: Option<PathBuf>,
}
impl UiState {
    fn new() -> Self {
        let file_dialog_filter =
            Box::new(|path: &Path| path.extension().is_some_and(|s| s == "ping"));
        let file_open_dialog = egui_file::FileDialog::open_file(None).filter(file_dialog_filter);
        Self {
            file_open_dialog,
            current_file: None,
        }
    }
    fn run(&mut self, ctx: &Context) {
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open...").clicked() {
                        ui.close_menu();
                        self.file_open_dialog.open();
                    }
                });
                ui.label(format!("Current File: {:?}", self.current_file));
            })
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            self.ping_map(ui);
        });
        match self.file_open_dialog.state() {
            egui_file::State::Open => {
                if self.file_open_dialog.show(ctx).selected() {
                    self.current_file = Some(self.file_open_dialog.path().unwrap())
                };
            }
            egui_file::State::Selected => {
                self.current_file = Some(self.file_open_dialog.path().unwrap())
            }
            _ => {}
        }
    }
    fn ping_map(&mut self, ui: &mut egui::Ui) {
        let size = ui.available_size();
        let (rect, response) = ui.allocate_exact_size(
            size,
            egui::Sense {
                click: false,
                drag: true,
                focusable: true,
            },
        );
        let callback = Arc::new(
            egui_wgpu::CallbackFn::new()
                .prepare(|device, queue, encoder, type_map| vec![])
                .paint(|cb_info, render_pass, type_map| {}),
        );
        ui.painter().add(egui::PaintCallback { rect, callback });
    }
}
