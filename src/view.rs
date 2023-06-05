use std::{path::Path, time::Duration};

use ipnet::Ipv4Net;
use iprange::IpRange;
use itertools::Itertools;
use tokio::{
    fs::File,
    io::{self, AsyncReadExt, BufReader},
    sync::{
        mpsc::{UnboundedReceiver, UnboundedSender},
        watch::{Receiver, Sender},
    },
};
use tracing::Level;
use wgpu::*;
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
    let (addr_tx, addr_rx) = tokio::sync::watch::channel(0u32);
    tokio::spawn(file_reader(args.filepath, instance_tx, addr_tx));

    let event_loop = EventLoop::new();
    let window = WindowBuilder::new().build(&event_loop).unwrap();

    let mut state = State::new(window, instance_rx, addr_rx).await;

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
                    Err(SurfaceError::Lost) => state.gpu.resize(state.gpu.size),
                    Err(SurfaceError::OutOfMemory) => *control_flow = ControlFlow::Exit,
                    Err(e) => tracing::warn!("{e:?}"),
                }
            }
            _ => {}
        }
    })
}

async fn file_reader(
    path: impl AsRef<Path>,
    instance_tx: UnboundedSender<Instance>,
    addr_tx: Sender<u32>,
) {
    let file = File::open(&path).await.unwrap();
    let mut buf_reader = BufReader::new(file);
    let nets = range_from_path(path).iter().collect_vec();
    let instances = nets.iter().flat_map(|net| net.hosts()).map(Instance::from);
    let poll_dur = Duration::from_millis(10);
    for mut instance in instances {
        addr_tx.send(instance.address).unwrap();
        let val = read_f32_wait(&mut buf_reader, poll_dur).await.unwrap();
        if val >= 0. {
            let color = (val / 0.5 * 255.).clamp(0., 255.) as u8;
            instance.color = u32::from_be_bytes([color, 255 - color, 255 - color, 255]);
            instance_tx.send(instance).unwrap()
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
    render_pipeline: RenderPipeline,
}
impl State {
    async fn new(
        window: Window,
        instance_rx: UnboundedReceiver<Instance>,
        addr_rx: Receiver<u32>,
    ) -> Self {
        let gpu = DeviceState::new(window).await;
        let pan_zoom = PanZoomState::new(&gpu, addr_rx);
        let ping_map = PingMapState::new(&gpu, instance_rx);

        let shader_module = gpu
            .device
            .create_shader_module(include_wgsl!("view/shader.wgsl"));

        let pipeline_layout_desc = PipelineLayoutDescriptor {
            label: Some("Render Pipeline Layout"),
            bind_group_layouts: &[&pan_zoom.bind_group_layout],
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
                format: gpu.config.format,
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
            gpu,
            pan_zoom,
            ping_map,
            render_pipeline,
        }
    }
    fn render(&mut self) -> Result<(), SurfaceError> {
        self.pan_zoom.update_buffer(&self.gpu);
        self.ping_map.update_buffer(&self.gpu);

        let output = self.gpu.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&TextureViewDescriptor::default());

        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        {
            let mut color_attachment = RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(Color::BLACK),
                    store: true,
                },
            };
            if self.gpu.sample_count > 1 {
                color_attachment.view = &self.gpu.multisample_framebuffer;
                color_attachment.resolve_target = Some(&view);
                color_attachment.ops.store = false;
            }
            let render_pass_descriptor = RenderPassDescriptor {
                label: Some("Render Pass"),
                depth_stencil_attachment: None,
                color_attachments: &mut [Some(color_attachment)],
            };
            let mut render_pass = encoder.begin_render_pass(&render_pass_descriptor);
            render_pass.set_pipeline(&self.render_pipeline);
            render_pass.set_bind_group(0, &self.pan_zoom.bind_group, &[]);
            render_pass.set_index_buffer(self.ping_map.index_buffer.slice(..), IndexFormat::Uint16);
            render_pass.set_vertex_buffer(0, self.ping_map.vertex_buffer.slice(..));
            for (len, buffer) in &self.ping_map.instance_buffers {
                render_pass.set_vertex_buffer(1, buffer.slice(..));
                render_pass.draw_indexed(0..self.ping_map.indicies.len() as _, 0, 0..*len as _);
            }
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

fn hilbert_decode(mut d: u32, bits: u32) -> [u32; 2] {
    let mut out = [0, 0];
    for i in 0..bits {
        let s = 2u32.pow(i);
        let rx = 1 & (d / 2);
        let ry = 1 & (d ^ rx);
        if ry == 0 {
            if rx == 1 {
                out[0] = s - 1 - out[0];
                out[1] = s - 1 - out[1];
            }
            let tmp = out[0];
            out[0] = out[1];
            out[1] = tmp;
        }
        out[0] += s * rx;
        out[1] += s * ry;
        d /= 4;
    }
    return out;
}

#[derive(Debug, clap::Args)]
pub struct Args {
    filepath: String,
}
