use std::{path::Path, time::Duration};

use ipnet::Ipv4Net;
use iprange::IpRange;
use itertools::Itertools;
use tokio::{
    fs::File,
    io::{self, AsyncReadExt, BufReader},
    sync::mpsc::{UnboundedReceiver, UnboundedSender},
};
use tracing::Level;
use winit::{
    event::Event,
    event_loop::{ControlFlow, EventLoop},
    window::{Window, WindowBuilder},
};

use crate::view::renderer::DeviceState;

use self::{
    pan_zoom::PanZoomState,
    ping_map::{Instance, PingMapState, Vertex},
};

mod pan_zoom;
mod ping_map;
mod renderer;

pub async fn main(args: Args) {
    tracing_subscriber::fmt::fmt()
        .with_max_level(Level::WARN)
        .init();

    let (instance_tx, instance_rx) = tokio::sync::mpsc::unbounded_channel::<Instance>();
    tokio::spawn(file_reader(args.filepath, instance_tx));

    let event_loop = EventLoop::new();
    let window = WindowBuilder::new().build(&event_loop).unwrap();

    let mut state = State::new(window, instance_rx).await;

    event_loop.run(move |event, _, control_flow| {
        use winit::event::{Event::*, WindowEvent::*};
        state.update(&event);
        match event {
            WindowEvent {
                event: CloseRequested,
                ..
            } => *control_flow = ControlFlow::Exit,
            RedrawRequested(..) => {
                let result = state.render();
                match result {
                    Ok(_) => {}
                    Err(wgpu::SurfaceError::Lost) => state.gpu.resize(state.gpu.size),
                    Err(wgpu::SurfaceError::OutOfMemory) => *control_flow = ControlFlow::Exit,
                    Err(e) => tracing::warn!("{e:?}"),
                }
            }
            _ => {}
        }
    })
}

async fn file_reader(path: impl AsRef<Path>, tx: UnboundedSender<Instance>) {
    let file = File::open(&path).await.unwrap();
    let mut buf_reader = BufReader::new(file);
    let nets = range_from_path(path).iter().collect_vec();
    let instances = nets.iter().flat_map(|net| net.hosts()).map(Instance::from);
    let poll_dur = Duration::from_millis(10);
    for instance in instances {
        let val = read_f32_wait(&mut buf_reader, poll_dur).await.unwrap();
        if val >= 0. {
            tx.send(instance).unwrap()
        }
    }
}

async fn read_f32_wait(buf_reader: &mut BufReader<File>, dur: Duration) -> io::Result<f32> {
    loop {
        match buf_reader.read_f32().await {
            Ok(val) => return Ok(val),
            Err(e) if e.kind() != io::ErrorKind::UnexpectedEof => return Err(e),
            _ => {}
        }
        tokio::time::sleep(dur).await;
    }
}

struct State {
    gpu: DeviceState,
    pan_zoom: PanZoomState,
    ping_map: PingMapState,
    render_pipeline: wgpu::RenderPipeline,
}
impl State {
    async fn new(window: Window, rx: UnboundedReceiver<Instance>) -> Self {
        let gpu = DeviceState::new(window).await;
        let pan_zoom = PanZoomState::new(&gpu);
        let ping_map = PingMapState::new(&gpu, rx);

        let shader = gpu
            .device
            .create_shader_module(wgpu::include_wgsl!("view/shader.wgsl"));
        let render_pipeline_layout =
            gpu.device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("Render Pipeline Layout"),
                    bind_group_layouts: &[&pan_zoom.bind_group_layout],
                    push_constant_ranges: &[],
                });
        let render_pipeline = gpu
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Render Pipeline"),
                layout: Some(&render_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: "vs_main",
                    buffers: &[Vertex::desc(), Instance::desc()],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: "fs_main",
                    targets: &[Some(wgpu::ColorTargetState {
                        format: gpu.config.format,
                        blend: Some(wgpu::BlendState::REPLACE),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    unclipped_depth: false,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    conservative: false,
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState {
                    count: 1,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                multiview: None,
            });

        Self {
            gpu,
            pan_zoom,
            ping_map,
            render_pipeline,
        }
    }
    fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        self.pan_zoom.update_buffer(&self.gpu);
        self.ping_map.update_buffer(&self.gpu);

        let output = self.gpu.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                depth_stencil_attachment: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 1.0,
                        }),
                        store: true,
                    },
                })],
            });
            render_pass.set_pipeline(&self.render_pipeline);
            render_pass.set_bind_group(0, &self.pan_zoom.bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.ping_map.vertex_buffer.slice(..));
            render_pass.set_vertex_buffer(1, self.ping_map.instance_buffer.slice(..));
            render_pass.set_index_buffer(
                self.ping_map.index_buffer.slice(..),
                wgpu::IndexFormat::Uint16,
            );
            render_pass.draw_indexed(
                0..self.ping_map.indicies.len() as _,
                0,
                0..self.ping_map.instances.len() as _,
            );
        }

        self.gpu.queue.submit(Some(encoder.finish()));
        output.present();
        Ok(())
    }

    fn update(&mut self, event: &Event<()>) {
        self.pan_zoom.update(&self.gpu, event);
        self.gpu.update(event);
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

#[derive(Debug, clap::Args)]
pub struct Args {
    filepath: String,
}
