use std::{net::Ipv4Addr, sync::Arc};

use bytemuck::{bytes_of, checked::cast_slice};
use egui::PaintCallbackInfo;
use tokio::sync::mpsc::UnboundedReceiver;
use type_map::concurrent::TypeMap;
use wgpu::{
    util::{BufferInitDescriptor, DeviceExt},
    *,
};

use crate::gpu::GpuState;

pub struct Widget {
    state_index: usize,
    instance_rx: UnboundedReceiver<Instance>,
    pan: [f32; 2],
    zoom: f32,
}

impl Widget {
    pub fn new(
        gpu: &GpuState,
        egui_renderer: &mut egui_wgpu::Renderer,
        instance_rx: UnboundedReceiver<Instance>,
    ) -> Self {
        let state = State::new(gpu);
        let state_index = Self::insert_state(&mut egui_renderer.paint_callback_resources, state);
        Self {
            instance_rx,
            state_index,
            pan: [0., 0.],
            zoom: 1.,
        }
    }
    pub fn show(&mut self, ui: &mut egui::Ui) {
        let size = ui.available_size();
        let (rect, response) = ui.allocate_exact_size(
            size,
            egui::Sense {
                click: false,
                drag: true,
                focusable: true,
            },
        );
        let mut scale = [
            1.0f32.min(rect.aspect_ratio().recip()),
            1.0f32.min(rect.aspect_ratio()),
        ];
        let last_zoom = self.zoom;
        if response.hovered() {
            self.zoom *= ui.ctx().input(|i| i.zoom_delta());
            self.zoom *= ui.ctx().input(|i| 1.005f32.powf(i.scroll_delta[1]));
            self.zoom = self.zoom.max(1.);
        }
        scale[0] *= self.zoom;
        scale[1] *= self.zoom;
        if let Some(pointer_pos) = ui.ctx().input(|i| i.pointer.hover_pos()) {
            let factor = self.zoom / last_zoom - 1.;
            self.pan[0] -= (pointer_pos[0] / rect.width() * 2. - 1.) / scale[0] * factor;
            self.pan[1] += (pointer_pos[1] / rect.height() * 2. - 1.) / scale[1] * factor;
        }
        self.pan[0] += response.drag_delta()[0] / rect.width() * 2. / scale[0];
        self.pan[1] -= response.drag_delta()[1] / rect.height() * 2. / scale[1];
        let get_state = self.state_getter_mut();
        let mut new_instances = vec![];
        while let Ok(i) = self.instance_rx.try_recv() {
            new_instances.push(i);
        }
        let pan = self.pan.clone();
        let prepare = move |device: &Device,
                            queue: &Queue,
                            _encoder: &mut CommandEncoder,
                            type_map: &mut TypeMap| {
            let state = get_state(type_map);
            state.update_pan_zoom(queue, pan, scale);
            if new_instances.len() > 0 {
                state.update_instances(device, queue, &new_instances);
            }
            vec![]
        };
        ui.painter().add(egui::PaintCallback {
            rect,
            callback: Arc::new(
                egui_wgpu::CallbackFn::new()
                    .prepare(prepare)
                    .paint(self.paint_fn()),
            ),
        });
    }
    fn paint_fn(
        &self,
    ) -> impl for<'a> Fn(PaintCallbackInfo, &mut wgpu::RenderPass<'a>, &'a TypeMap) {
        let get_state = self.state_getter();
        move |_, render_pass, type_map| {
            get_state(type_map).paint(render_pass);
        }
    }
    /// Return a function that will retrive OUR state from the typemap
    fn state_getter(&self) -> impl for<'a> Fn(&'a TypeMap) -> &'a State {
        let index = self.state_index;
        move |tm| &tm.get::<Vec<State>>().unwrap()[index]
    }
    /// Return a function that will retrive OUR state from the typemap
    fn state_getter_mut(&self) -> impl for<'a> Fn(&'a mut TypeMap) -> &'a mut State {
        let index = self.state_index;
        move |tm| &mut tm.get_mut::<Vec<State>>().unwrap()[index]
    }
    /// Insert a state into the given typemap, and return the state index
    fn insert_state(type_map: &mut TypeMap, state: State) -> usize {
        let states = type_map.entry::<Vec<State>>().or_insert(vec![]);
        let state_index = states.len();
        states.push(state);
        state_index
    }
}

struct State {
    render_pipeline: RenderPipeline,
    pan_zoom_buffer: Buffer,
    pan_zoom_bind_group: BindGroup,
    vertex_buffer: Buffer,
    index_buffer: Buffer,
    instance_buffers: Vec<(Buffer, usize)>,
    max_buffer_size: BufferAddress,
}
impl State {
    fn update_instances(&mut self, device: &Device, queue: &Queue, mut instances: &[Instance]) {
        const INSTANCE_SIZE: BufferAddress = std::mem::size_of::<Instance>() as _;
        loop {
            let (buffer, num_occupied) = self.instance_buffers.last_mut().unwrap();
            let remaining_slots = (self.max_buffer_size / INSTANCE_SIZE) as usize - *num_occupied;
            let offset = *num_occupied as BufferAddress * INSTANCE_SIZE;
            if instances.len() < remaining_slots {
                queue.write_buffer(&buffer, offset, cast_slice(instances));
                *num_occupied += instances.len();
                break;
            } else {
                queue.write_buffer(&buffer, offset, cast_slice(&instances[..remaining_slots]));
                *num_occupied += remaining_slots;
                instances = &instances[remaining_slots..];
                let new_buffer = device.create_buffer(&BufferDescriptor {
                    label: Some(&format!("Instance Buffer {}", self.instance_buffers.len())),
                    size: self.max_buffer_size,
                    usage: BufferUsages::COPY_DST | BufferUsages::VERTEX,
                    mapped_at_creation: false,
                });
                self.instance_buffers.push((new_buffer, 0));
            }
        }
    }
    fn update_pan_zoom(&mut self, queue: &Queue, pan: [f32; 2], zoom: [f32; 2]) {
        let uniform = PanZoomUniform { pan, scale: zoom };
        queue.write_buffer(&self.pan_zoom_buffer, 0, bytes_of(&uniform));
    }
    fn paint<'a>(&'a self, render_pass: &mut RenderPass<'a>) {
        render_pass.set_pipeline(&self.render_pipeline);
        render_pass.set_bind_group(0, &self.pan_zoom_bind_group, &[]);
        render_pass.set_index_buffer(self.index_buffer.slice(..), IndexFormat::Uint16);
        render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        for (buffer, num_occupied) in &self.instance_buffers {
            render_pass.set_vertex_buffer(1, buffer.slice(..));
            render_pass.draw_indexed(0..INDICES.len() as _, 0, 0..*num_occupied as _);
        }
    }
    fn new(gpu: &GpuState) -> Self {
        let max_buffer_size = gpu.device.limits().max_buffer_size;
        let shader_module = gpu
            .device
            .create_shader_module(include_wgsl!("view/shader.wgsl"));
        let pan_zoom_buffer = gpu.device.create_buffer_init(&BufferInitDescriptor {
            label: Some("Pan Zoom Buffer"),
            contents: bytes_of(&PanZoomUniform::default()),
            usage: BufferUsages::COPY_DST | BufferUsages::UNIFORM,
        });
        let vertex_buffer = gpu.device.create_buffer_init(&BufferInitDescriptor {
            label: Some("Vertex Buffer"),
            contents: bytemuck::cast_slice(VERTICES),
            usage: BufferUsages::VERTEX,
        });
        let index_buffer = gpu.device.create_buffer_init(&BufferInitDescriptor {
            label: Some("Index Buffer"),
            contents: bytemuck::cast_slice(INDICES),
            usage: BufferUsages::INDEX,
        });
        let instance_buffers = vec![(
            gpu.device.create_buffer(&BufferDescriptor {
                label: Some("Instance Buffer 0"),
                size: max_buffer_size,
                usage: BufferUsages::COPY_DST | BufferUsages::VERTEX,
                mapped_at_creation: false,
            }),
            0,
        )];
        let pan_zoom_bind_group_layout =
            gpu.device
                .create_bind_group_layout(&BindGroupLayoutDescriptor {
                    entries: &[BindGroupLayoutEntry {
                        binding: 0,
                        visibility: ShaderStages::VERTEX,
                        ty: BindingType::Buffer {
                            ty: BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    }],
                    label: Some("Pan Zoom Bind Group Layout"),
                });
        let pan_zoom_bind_group = gpu.device.create_bind_group(&BindGroupDescriptor {
            layout: &pan_zoom_bind_group_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: pan_zoom_buffer.as_entire_binding(),
            }],
            label: Some("Pan Zoom Bind Group"),
        });
        let pipeline_layout_desc = PipelineLayoutDescriptor {
            label: Some("Render Pipeline Layout"),
            bind_group_layouts: &[&pan_zoom_bind_group_layout],
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
            render_pipeline,
            pan_zoom_buffer,
            pan_zoom_bind_group,
            vertex_buffer,
            index_buffer,
            instance_buffers,
            max_buffer_size,
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Instance {
    pub address: u32,
    pub color: u32,
}
impl Instance {
    const ATTRS: [VertexAttribute; 2] = vertex_attr_array![1 => Uint32, 2 => Uint32];
    pub fn desc() -> VertexBufferLayout<'static> {
        VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as BufferAddress,
            step_mode: VertexStepMode::Instance,
            attributes: &Self::ATTRS,
        }
    }
}
impl From<Ipv4Addr> for Instance {
    fn from(addr: Ipv4Addr) -> Self {
        Self {
            address: u32::from_be_bytes(addr.octets()),
            color: u32::from_be_bytes([255, 255, 255, 255]),
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    position: [f32; 2],
}
impl Vertex {
    const ATTRS: [VertexAttribute; 1] = vertex_attr_array![0 => Float32x2];
    pub fn desc() -> VertexBufferLayout<'static> {
        VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as BufferAddress,
            step_mode: VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}

const CORNER: f32 = 0.00001525878;
const INDICES: &[u16] = &[0, 1, 2, 2, 1, 3];
const VERTICES: &[Vertex] = &[
    Vertex {
        position: [-CORNER, -CORNER],
    },
    Vertex {
        position: [CORNER, -CORNER],
    },
    Vertex {
        position: [-CORNER, CORNER],
    },
    Vertex {
        position: [CORNER, CORNER],
    },
];

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PanZoomUniform {
    pan: [f32; 2],
    scale: [f32; 2],
}
impl Default for PanZoomUniform {
    fn default() -> Self {
        Self {
            pan: [0., 0.],
            scale: [1., 1.],
        }
    }
}
