use std::{
    fs::File,
    io::{BufReader, Read},
    path::Path,
};

use ipnet::Ipv4Net;
use iprange::IpRange;
use itertools::Itertools;
use tracing::Level;
use wgpu::{util::DeviceExt, VertexAttribute};
use winit::{
    dpi::PhysicalPosition,
    event::{ElementState, Event, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};

use crate::view::renderer::Renderer;

// mod pan_zoom;
// mod ping_map;
mod renderer;

pub async fn main(args: Args) {
    tracing_subscriber::fmt::fmt()
        .with_max_level(Level::WARN)
        .init();
    let mut file = BufReader::new(File::open(&args.filepath).unwrap());
    let range = range_from_path(args.filepath);
    let bools = read_file(&mut file);

    let addrs = range
        .iter()
        .flat_map(|net| net.hosts())
        .zip(bools)
        .filter_map(|(h, b)| b.then_some(h))
        .map(|h| Instance {
            hilb: u32::from_be_bytes(h.octets()),
        })
        .collect_vec();

    let event_loop = EventLoop::new();
    let window = WindowBuilder::new().build(&event_loop).unwrap();

    let mut rend = Renderer::new(window).await;
    let mut ping_map = State::new(&rend, addrs);

    use winit::event::WindowEvent::*;
    event_loop.run(move |event, _, control_flow| match event {
        Event::WindowEvent {
            ref event,
            window_id,
        } if window_id == rend.window.id() && !ping_map.input(&rend, event) => match event {
            CloseRequested => *control_flow = ControlFlow::Exit,
            ScaleFactorChanged { new_inner_size, .. } => rend.resize(**new_inner_size),
            Resized(physical_size) => rend.resize(*physical_size),
            _ => {}
        },
        Event::RedrawRequested(window_id) if window_id == rend.window.id() => {
            ping_map.update(&rend);
            match ping_map.render(&rend) {
                Ok(_) => {}
                Err(wgpu::SurfaceError::Lost) => rend.resize(rend.size),
                Err(wgpu::SurfaceError::OutOfMemory) => *control_flow = ControlFlow::Exit,
                Err(e) => tracing::warn!("{e:?}"),
            }
        }
        Event::MainEventsCleared => {
            rend.window.request_redraw();
        }
        _ => {}
    })
}

struct State {
    render_pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    num_indices: u32,
    instances: Vec<Instance>,
    instance_buffer: wgpu::Buffer,
    pan_zoom_uniform: PanZoomUniform,
    pan_zoom_buffer: wgpu::Buffer,
    pan_zoom_bind_group: wgpu::BindGroup,
    mouse_down: bool,
    last_position: Option<(f64, f64)>,
}
impl State {
    fn new(renderer: &Renderer, instances: Vec<Instance>) -> Self {
        let vertex_buffer = renderer
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Vertex Buffer"),
                contents: bytemuck::cast_slice(VERTICES),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let index_buffer = renderer
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Index Buffer"),
                contents: bytemuck::cast_slice(INDICES),
                usage: wgpu::BufferUsages::INDEX,
            });
        let instance_buffer =
            renderer
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Instance Buffer"),
                    contents: bytemuck::cast_slice(&instances[..]),
                    usage: wgpu::BufferUsages::VERTEX,
                });
        let pan_zoom_uniform = PanZoomUniform::default();
        let pan_zoom_buffer =
            renderer
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Camera Buffer"),
                    contents: bytemuck::cast_slice(&[pan_zoom_uniform]),
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                });

        let pan_zoom_bind_group_layout =
            renderer
                .device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    entries: &[wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    }],
                    label: Some("Pan Zoom Bind Group Layout"),
                });

        let pan_zoom_bind_group = renderer
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                layout: &pan_zoom_bind_group_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: pan_zoom_buffer.as_entire_binding(),
                }],
                label: Some("Pan Zoom Bind Group"),
            });

        let shader = renderer
            .device
            .create_shader_module(wgpu::include_wgsl!("view/shader.wgsl"));
        let render_pipeline_layout =
            renderer
                .device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("Render Pipeline Layout"),
                    bind_group_layouts: &[&pan_zoom_bind_group_layout],
                    push_constant_ranges: &[],
                });
        let render_pipeline =
            renderer
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
                            format: renderer.config.format,
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
            mouse_down: false,
            render_pipeline,
            vertex_buffer,
            index_buffer,
            num_indices: INDICES.len() as u32,
            instances,
            instance_buffer,
            pan_zoom_uniform,
            pan_zoom_buffer,
            pan_zoom_bind_group,
            last_position: None,
        }
    }
    fn render(&mut self, renderer: &Renderer) -> Result<(), wgpu::SurfaceError> {
        let output = renderer.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = renderer
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
            render_pass.set_bind_group(0, &self.pan_zoom_bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            render_pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
            render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
            render_pass.draw_indexed(0..self.num_indices, 0, 0..self.instances.len() as _);
        }
        renderer.queue.submit(Some(encoder.finish()));
        output.present();
        Ok(())
    }

    fn input(&mut self, rend: &Renderer, event: &WindowEvent) -> bool {
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                if let Some((last_x, last_y)) = self.last_position {
                    if self.mouse_down {
                        let dx = (position.x - last_x)
                            / rend.size.width as f64
                            / self.pan_zoom_uniform.zoom[0] as f64
                            * 2.;
                        let dy = (position.y - last_y)
                            / rend.size.height as f64
                            / self.pan_zoom_uniform.zoom[1] as f64
                            * 2.;
                        self.pan_zoom_uniform.pan[0] += dx as f32;
                        self.pan_zoom_uniform.pan[1] -= dy as f32;
                    }
                }
                self.last_position = Some((position.x, position.y))
            }
            WindowEvent::MouseWheel {
                delta: MouseScrollDelta::LineDelta(_, y),
                ..
            } => {
                self.pan_zoom_uniform.zoom[0] *= 1.1f32.powf(*y);
                self.pan_zoom_uniform.zoom[1] *= 1.1f32.powf(*y);
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => self.mouse_down = true,
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => self.mouse_down = false,
            _ => return false,
        }
        true
    }

    fn update(&mut self, rend: &Renderer) {
        rend.queue.write_buffer(
            &self.pan_zoom_buffer,
            0,
            bytemuck::cast_slice(&[self.pan_zoom_uniform]),
        );
    }
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Instance {
    hilb: u32,
}
impl Instance {
    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![2 => Uint32],
        }
    }
}
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct PanZoomUniform {
    pan: [f32; 2],
    zoom: [f32; 2],
}
impl Default for PanZoomUniform {
    fn default() -> Self {
        Self {
            pan: [0., 0.],
            zoom: [1., 1.],
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 3],
    uv: [f32; 2],
}
impl Vertex {
    const ATTRS: [VertexAttribute; 2] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2];
    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}

const INDICES: &[u16] = &[0, 1, 2, 2, 1, 3];
const VERTICES: &[Vertex] = &[
    Vertex {
        position: [-1.0, -1.0, 0.0],
        uv: [0.0, 0.0],
    },
    Vertex {
        position: [1.0, -1.0, 0.0],
        uv: [0.0, 1.0],
    },
    Vertex {
        position: [-1.0, 1.0, 0.0],
        uv: [1.0, 0.0],
    },
    Vertex {
        position: [1.0, 1.0, 0.0],
        uv: [1.0, 1.0],
    },
];

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

fn read_file(file: &mut BufReader<File>) -> Vec<bool> {
    let mut bools = vec![];
    let mut buf = [0u8; 4];
    while file.read_exact(&mut buf).is_ok() {
        let val = f32::from_be_bytes(buf);
        bools.push(val >= 0.);
    }
    bools
}

#[derive(Debug, clap::Args)]
pub struct Args {
    filepath: String,
}
