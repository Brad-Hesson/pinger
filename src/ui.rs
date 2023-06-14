use std::{
    iter,
    path::{Path, PathBuf},
};

use winit::{event::WindowEvent, event_loop::ControlFlow};

use crate::gpu::GpuState;
use crate::ping_map;

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

    let mut ui_state = UiState::new(&gpu, &mut egui_renderer);

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
                    gpu.resize(size);
                    window.request_redraw();
                }
                WindowEvent::ScaleFactorChanged { new_inner_size, .. } => {
                    gpu.resize(*new_inner_size);
                    window.request_redraw();
                }
                WindowEvent::CloseRequested => {
                    *control_flow = ControlFlow::Exit;
                }
                _ => {}
            };
        }
        winit::event::Event::RedrawRequested(..) => {
            let Ok((surface, view)) = gpu.get_surface_texture() else {
                return
            };

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
            surface.present();
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

pub struct UiState {
    file_open_dialog: FileDialog,
    ping_map: ping_map::Widget,
}
impl UiState {
    pub fn new(gpu: &GpuState, egui_renderer: &mut egui_wgpu::Renderer) -> Self {
        let ping_map = ping_map::Widget::new(gpu, egui_renderer);
        Self {
            file_open_dialog: FileDialog::new(),
            ping_map,
        }
    }
    pub fn run(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open...").clicked() {
                        ui.close_menu();
                        self.file_open_dialog.open();
                    }
                });
                if let Some(ref path) = self.file_open_dialog.path {
                    ui.label(format!(
                        "Current File: {:?}",
                        path.file_name().unwrap().to_str().unwrap()
                    ));
                }
            })
        });
        egui::CentralPanel::default()
            .frame(egui::Frame {
                inner_margin: egui::Margin::same(0.),
                outer_margin: egui::Margin::same(0.),
                rounding: egui::Rounding::none(),
                shadow: egui::epaint::Shadow::NONE,
                fill: egui::Color32::TRANSPARENT,
                stroke: egui::Stroke::NONE,
            })
            .show(ctx, |ui| {
                self.ping_map.show(ui);
            });
        if self.file_open_dialog.show(ctx).just_selected {
            self.ping_map
                .open_file(self.file_open_dialog.path.as_ref().unwrap());
        }
    }
}

struct FileDialog {
    dialog: egui_file::FileDialog,
    path: Option<PathBuf>,
    just_selected: bool,
}
impl FileDialog {
    fn new() -> Self {
        let filter = |path: &Path| path.extension().is_some_and(|s| s == "ping");
        let dialog = egui_file::FileDialog::open_file(None).filter(Box::new(filter));
        Self {
            dialog,
            path: None,
            just_selected: false,
        }
    }
    fn show(&mut self, ctx: &egui::Context) -> &mut Self {
        self.just_selected = false;
        if self.dialog.show(ctx).selected() {
            self.just_selected = true;
            self.path = self.dialog.path();
        };
        self
    }
    fn open(&mut self) {
        self.dialog.open();
    }
}
