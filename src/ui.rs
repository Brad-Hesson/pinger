use std::{
    iter,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use egui::Context;
use ipnet::Ipv4Net;
use iprange::IpRange;
use itertools::Itertools;
use tokio::{
    fs::File,
    io::{AsyncReadExt, BufReader},
    sync::mpsc::{UnboundedReceiver, UnboundedSender},
};
use wgpu::*;
use winit::{event::WindowEvent, event_loop::ControlFlow, window::Window};

use crate::{
    gpu::GpuState,
    view::ping_map::{Instance, PingMapState, Vertex},
};

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
    let (instance_tx, instance_rx) = tokio::sync::mpsc::unbounded_channel::<Instance>();
    let render_state = RenderState::new(&gpu, instance_rx).await;
    egui_renderer.paint_callback_resources.insert(render_state);
    tokio::spawn(file_reader("0.0.0.0-0.ping", instance_tx));

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

async fn file_reader(path: impl AsRef<Path>, instance_tx: UnboundedSender<Instance>) {
    let file = File::open(&path).await.unwrap();
    let mut buf_reader = BufReader::new(file);
    let nets = range_from_path(path).iter().collect_vec();
    let instances = nets.iter().flat_map(|net| net.hosts()).map(Instance::from);
    let poll_dur = Duration::from_millis(10);
    for mut instance in instances {
        let val = read_f32_wait(&mut buf_reader, poll_dur).await.unwrap();
        if val >= 0. {
            let color = (val / 0.5 * 255.).clamp(0., 255.) as u8;
            instance.color = u32::from_be_bytes([color, 255 - color, 255 - color, 255]);
            instance_tx.send(instance).unwrap()
        }
    }
}

async fn read_f32_wait(buf_reader: &mut BufReader<File>, dur: Duration) -> std::io::Result<f32> {
    loop {
        match buf_reader.read_f32().await {
            Ok(val) => return Ok(val),
            Err(e) if e.kind() != std::io::ErrorKind::UnexpectedEof => return Err(e),
            _ => {}
        }
        tokio::time::sleep(dur).await;
    }
}

fn range_from_path(path: impl AsRef<Path>) -> IpRange<Ipv4Net> {
    let filename = path.as_ref().file_stem().unwrap().to_str().unwrap();
    let mut range = IpRange::<Ipv4Net>::new();
    for s in filename.split('_') {
        let s = s.replace('-', "/").parse().unwrap();
        range.add(s);
    }
    range.simplify();
    range
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
        // let zoom_delta = ui.ctx().input(|i| i.zoom_delta());
        let callback = Arc::new(
            egui_wgpu::CallbackFn::new()
                .prepare(|device, queue, encoder, type_map| {
                    let r = type_map.get_mut::<RenderState>().unwrap();
                    r.prepare(device, queue, encoder);
                    vec![]
                })
                .paint(|cb_info, render_pass, type_map| {
                    let r = type_map.get::<RenderState>().unwrap();
                    r.paint(render_pass);
                }),
        );
        ui.painter().add(egui::PaintCallback { rect, callback });
    }
}

struct RenderState {
    ping_map: PingMapState,
    render_pipeline: RenderPipeline,
}
impl RenderState {
    async fn new(gpu: &GpuState, instance_rx: UnboundedReceiver<Instance>) -> Self {
        let ping_map = PingMapState::new(&gpu, instance_rx);

        let shader_module = gpu
            .device
            .create_shader_module(include_wgsl!("view/shader.wgsl"));

        let pipeline_layout_desc = PipelineLayoutDescriptor {
            label: Some("Render Pipeline Layout"),
            bind_group_layouts: &[],
            // bind_group_layouts: &[&pan_zoom.bind_group_layout],
            push_constant_ranges: &[],
        };
        let render_pipeline_layout = gpu.device.create_pipeline_layout(&pipeline_layout_desc);

        let vertex_state = VertexState {
            module: &shader_module,
            entry_point: "vs_main",
            buffers: &[Vertex::desc(), Instance::desc()],
        };
        let primitive_state = PrimitiveState {
            topology: PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: FrontFace::Ccw,
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: PolygonMode::Fill,
            conservative: false,
        };
        let fragment_state = FragmentState {
            module: &shader_module,
            entry_point: "fs_main",
            targets: &[Some(ColorTargetState {
                format: gpu.surface_config.format,
                blend: Some(BlendState::REPLACE),
                write_mask: ColorWrites::ALL,
            })],
        };
        let multisample_state = MultisampleState {
            count: gpu.sample_count,
            mask: !0,
            alpha_to_coverage_enabled: false,
        };
        let render_pipeline_desc = RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: vertex_state,
            fragment: Some(fragment_state),
            primitive: primitive_state,
            depth_stencil: None,
            multisample: multisample_state,
            multiview: None,
        };
        let render_pipeline = gpu.device.create_render_pipeline(&render_pipeline_desc);

        Self {
            ping_map,
            render_pipeline,
        }
    }
    fn prepare(&mut self, device: &Device, queue: &Queue, encoder: &mut CommandEncoder) {
        self.ping_map.prepare(device);
    }
    fn paint<'a>(&'a self, render_pass: &mut RenderPass<'a>) {
        render_pass.set_pipeline(&self.render_pipeline);
        // render_pass.set_bind_group(0, &self.pan_zoom.bind_group, &[]);
        render_pass.set_index_buffer(self.ping_map.index_buffer.slice(..), IndexFormat::Uint16);
        render_pass.set_vertex_buffer(0, self.ping_map.vertex_buffer.slice(..));
        for (len, buffer) in &self.ping_map.instance_buffers {
            render_pass.set_vertex_buffer(1, buffer.slice(..));
            render_pass.draw_indexed(0..self.ping_map.indicies.len() as _, 0, 0..*len as _);
        }
    }
}
