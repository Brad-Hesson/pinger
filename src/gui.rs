use std::path::{Path, PathBuf};

use egui::Context;
use egui_wgpu::Renderer;

use crate::{gpu::GpuState, ping_map};

pub struct UiState {
    file_open_dialog: Option<egui_file::FileDialog>,
    current_file: Option<PathBuf>,
    ping_map: ping_map::Widget,
}
impl UiState {
    pub fn new(gpu: &GpuState, egui_renderer: &mut Renderer) -> Self {
        let ping_map = ping_map::Widget::new(gpu, egui_renderer);
        Self {
            file_open_dialog: None,
            current_file: None,
            ping_map,
        }
    }
    pub fn run(&mut self, ctx: &Context) {
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open...").clicked() {
                        ui.close_menu();
                        self.open_file_open_dialog();
                    }
                });
                if let Some(ref path) = self.current_file {
                    ui.label(format!(
                        "Current File: {:?}",
                        path.file_name().unwrap().to_str().unwrap()
                    ));
                }
            })
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            self.ping_map.show(ui);
        });
        if let Some(ref mut dialog) = self.file_open_dialog {
            match dialog.state() {
                egui_file::State::Open => {
                    dialog.show(ctx);
                }
                egui_file::State::Selected => {
                    let path = dialog.path().unwrap();
                    self.current_file = Some(path.clone());
                    self.ping_map.open_file(path);
                    self.file_open_dialog.take();
                }
                _ => {}
            }
        }
    }
    fn open_file_open_dialog(&mut self) {
        let mut file_open_dialog =
            egui_file::FileDialog::open_file(None).filter(Box::new(|path: &Path| {
                path.extension().is_some_and(|s| s == "ping")
            }));
        file_open_dialog.open();
        self.file_open_dialog = Some(file_open_dialog);
    }
}
