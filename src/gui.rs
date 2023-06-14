use std::path::{Path, PathBuf};

use egui::Context;
use egui_wgpu::Renderer;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::{
    gpu::GpuState,
    ping_map::{self, Instance},
};

pub struct UiState {
    file_open_dialog: egui_file::FileDialog,
    current_file: Option<PathBuf>,
    ping_map: ping_map::Widget,
}
impl UiState {
    pub fn new(
        gpu: &GpuState,
        egui_renderer: &mut Renderer,
        instance_rx: UnboundedReceiver<Instance>,
    ) -> Self {
        let file_dialog_filter =
            Box::new(|path: &Path| path.extension().is_some_and(|s| s == "ping"));
        let file_open_dialog = egui_file::FileDialog::open_file(None).filter(file_dialog_filter);
        let ping_map = ping_map::Widget::new(gpu, egui_renderer, instance_rx);
        Self {
            file_open_dialog,
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
                        self.file_open_dialog.open();
                    }
                });
                ui.label(format!("Current File: {:?}", self.current_file));
            })
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            self.ping_map.show(ui);
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
}
