use std::path::{Path, PathBuf};

use egui::{epaint::Shadow, Color32, Context, Margin, Rounding, Stroke};
use egui_wgpu::Renderer;

use crate::{gpu::GpuState, ping_map};

pub struct UiState {
    file_open_dialog: FileDialog,
    ping_map: ping_map::Widget,
}
impl UiState {
    pub fn new(gpu: &GpuState, egui_renderer: &mut Renderer) -> Self {
        let ping_map = ping_map::Widget::new(gpu, egui_renderer);
        Self {
            file_open_dialog: FileDialog::new(),
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
                inner_margin: Margin::same(0.),
                outer_margin: Margin::same(0.),
                rounding: Rounding::none(),
                shadow: Shadow::NONE,
                fill: Color32::TRANSPARENT,
                stroke: Stroke::NONE,
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
    fn show(&mut self, ctx: &Context) -> &mut Self {
        self.just_selected = false;
        if self.dialog.show(ctx).selected() {
            self.just_selected = true;
            self.path = self.dialog.path()
        };
        self
    }
    fn open(&mut self) {
        self.dialog.open();
    }
}
