use std::{iter, path::Path, time::Duration};

use ipnet::Ipv4Net;
use iprange::IpRange;
use itertools::Itertools;
use tokio::{
    fs::File,
    io::{AsyncReadExt, BufReader},
    sync::mpsc::UnboundedSender,
};
use winit::{event::WindowEvent, event_loop::ControlFlow};

use crate::{gpu::GpuState, gui::UiState, ping_map::Instance};

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

    let (instance_tx, instance_rx) = tokio::sync::mpsc::unbounded_channel::<Instance>();
    let mut ui_state = UiState::new(&gpu, &mut egui_renderer, instance_rx);
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
