use std::{net::Ipv4Addr, path::Path, sync::Arc, time::Duration};

use bytemuck::bytes_of;
use egui::{vec2, PaintCallbackInfo, Vec2};
use ipnet::Ipv4Net;
use iprange::IpRange;
use itertools::Itertools;
use tokio::{
    fs::File,
    io::{AsyncReadExt, BufReader},
    sync::mpsc::{UnboundedReceiver, UnboundedSender},
    task::JoinHandle,
};
use type_map::concurrent::TypeMap;
use wgpu::{
    util::{BufferInitDescriptor, DeviceExt},
    *,
};

use crate::{gpu::GpuState, wgpu_ext::BufferVec};

pub struct Widget {
    state_index: usize,
    instance_rx: Option<UnboundedReceiver<Instance>>,
    file_reader_handle: Option<JoinHandle<()>>,
    reset: bool,
    pan: Vec2,
    zoom: f32,
}

impl Widget {
    pub fn new(gpu: &GpuState, egui_renderer: &mut egui_wgpu::Renderer) -> Self {
        let state = State::new(gpu, 16 - 6);
        let state_index = Self::insert_state(&mut egui_renderer.paint_callback_resources, state);
        Self {
            instance_rx: None,
            state_index,
            pan: vec2(0., 0.),
            zoom: 1.,
            file_reader_handle: None,
            reset: false,
        }
    }
    pub fn show(&mut self, ui: &mut egui::Ui) {
        let size = ui.available_size();
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());

        let (pan, zoom) = self.handle_input(ui, rect, &response);

        let mut new_instances = vec![];
        if let Some(ref mut rx) = self.instance_rx {
            while let Ok(i) = rx.try_recv() {
                new_instances.push(i);
            }
        }

        let reset = self.reset;
        self.reset = false;

        let get_state = self.state_getter_mut();
        let prepare = move |device: &Device,
                            queue: &Queue,
                            encoder: &mut CommandEncoder,
                            type_map: &mut TypeMap| {
            let span = tracing::trace_span!("Prepare Pingmap");
            let _span = span.enter();
            let state = get_state(type_map);
            state.update_pan_zoom(queue, pan, zoom);
            if reset {
                state.reset();
            }
            if !new_instances.is_empty() {
                state.update_instances(device, queue, encoder, &new_instances);
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
    fn handle_input(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        response: &egui::Response,
    ) -> ([f32; 2], [f32; 2]) {
        if ui.ctx().input(|i| i.key_pressed(egui::Key::Space)) {
            self.zoom = 1.;
            self.pan = vec2(0., 0.);
        }
        // scale x or y down to make it render square
        let mut scale = vec2(
            1.0f32.min(rect.aspect_ratio().recip()),
            1.0f32.min(rect.aspect_ratio()),
        );
        // save the prev zoom level
        let last_zoom = self.zoom;
        // if the cursor is hovering over, then accept zoom inputs
        if response.hovered() {
            self.zoom *= ui.ctx().input(|i| i.zoom_delta());
            self.zoom *= ui.ctx().input(|i| 1.005f32.powf(i.scroll_delta.y));
            if response.double_clicked() {
                self.zoom *= 4.;
            }
            self.zoom = self.zoom.max(1.);
        }
        // apply the zoom to the scale vec
        scale *= self.zoom;
        let screen_to_uv = vec2(2., 2.) / rect.size() / scale;
        // calculate how much to pan to make the zooming centered on the cursor
        if let Some(pointer_pos) = ui.ctx().input(|i| i.pointer.hover_pos()) {
            let factor = self.zoom / last_zoom - 1.;
            self.pan -= (pointer_pos - rect.center()) * factor * screen_to_uv;
        }
        // apply pointer dragging to the pan vec
        self.pan += response.drag_delta() * screen_to_uv;
        let mut pan = self.pan;
        // invert y because of coordinate differences
        pan.y *= -1.;
        (pan.into(), scale.into())
    }
    pub fn open_file(&mut self, path: impl AsRef<Path>) {
        self.zoom = 1.;
        self.pan = vec2(0., 0.);
        if let Some(handle) = self.file_reader_handle.take() {
            handle.abort();
            self.reset = true;
        }
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        self.instance_rx = Some(rx);
        let handle = tokio::spawn(file_reader(path.as_ref().to_path_buf(), tx));
        self.file_reader_handle = Some(handle);
    }
    fn paint_fn(
        &self,
    ) -> impl for<'a> Fn(PaintCallbackInfo, &mut wgpu::RenderPass<'a>, &'a TypeMap) {
        let get_state = self.state_getter();
        move |_, render_pass, type_map| {
            let span = tracing::trace_span!("Paint Pingmap");
            let _span = span.enter();
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
    blocks: Vec<Option<Block>>,
    texture_bind_group_layout: BindGroupLayout,
    bits_per_block: u32,
    bits_per_block_bind_group: Arc<BindGroup>,
    bits_per_block_bind_group_layout: BindGroupLayout,
    next_to_clear: usize,
}
impl State {
    fn update_instances(
        &mut self,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        instances: &[Instance],
    ) {
        let modified = self.push_instances(device, queue, instances);
        for i in &modified {
            let bits_per_block_bind_group = self.bits_per_block_bind_group.clone();
            self.get_block_mut(device, *i)
                .render(encoder, &bits_per_block_bind_group);
        }
        if let Some(last) = modified.last() {
            for i in self.next_to_clear..*last {
                if let Some(block) = &mut self.blocks[i] {
                    block.instance_buffers.clear();
                }
            }
            self.next_to_clear = *last;
        }
    }
    pub fn push_instances(
        &mut self,
        device: &Device,
        queue: &Queue,
        instances: &[Instance],
    ) -> Vec<usize> {
        let block_size = 2usize.pow(self.bits_per_block).pow(2);
        let instance_groups = instances
            .into_iter()
            .group_by(|i| i.address as usize / block_size);
        let mut modified = vec![];
        for (block_index, instances) in instance_groups.into_iter() {
            modified.push(block_index);
            let block = self.get_block_mut(device, block_index);
            let instances = instances.copied().collect::<Vec<_>>();
            block.instance_buffers.extend(device, queue, &instances);
        }
        modified
    }
    fn update_pan_zoom(&mut self, queue: &Queue, pan: [f32; 2], scale: [f32; 2]) {
        queue.write_buffer(
            &self.pan_zoom_buffer,
            0,
            bytes_of(&PanZoomUniform { pan, scale }),
        );
    }
    fn paint<'a>(&'a self, render_pass: &mut RenderPass<'a>) {
        render_pass.set_pipeline(&self.render_pipeline);
        render_pass.set_bind_group(0, &self.bits_per_block_bind_group, &[]);
        render_pass.set_bind_group(1, &self.pan_zoom_bind_group, &[]);
        for block in self.blocks.iter().filter_map(|m| m.as_ref()) {
            render_pass.set_bind_group(2, &block.block_index_bind_group, &[]);
            render_pass.set_bind_group(3, &block.texture_bind_group, &[]);
            render_pass.draw(0..6, 0..1);
        }
    }
    fn get_block_mut(&mut self, device: &Device, index: usize) -> &mut Block {
        let maybe_block = &mut self.blocks[index];
        if maybe_block.is_none() {
            *maybe_block = Some(Block::new(
                device,
                index as _,
                &self.texture_bind_group_layout,
                &self.bits_per_block_bind_group_layout,
                2u32.pow(self.bits_per_block),
            ));
        }
        maybe_block.as_mut().unwrap()
    }
    fn reset(&mut self) {
        for block in &mut self.blocks {
            *block = None;
        }
        self.next_to_clear = 0;
    }
    fn new(gpu: &GpuState, bits_per_block: u32) -> Self {
        let shader_module = gpu
            .device
            .create_shader_module(include_wgsl!("shader.wgsl"));
        let bits_per_block_buffer = gpu.device.create_buffer_init(&BufferInitDescriptor {
            label: Some("Bits per Block Buffer"),
            contents: bytes_of(&bits_per_block),
            usage: BufferUsages::COPY_DST | BufferUsages::UNIFORM,
        });
        let bits_per_block_bind_group_layout =
            gpu.device
                .create_bind_group_layout(&BindGroupLayoutDescriptor {
                    entries: &[BindGroupLayoutEntry {
                        binding: 0,
                        visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
                        ty: BindingType::Buffer {
                            ty: BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    }],
                    label: Some("Bits per Block Group Layout"),
                });
        let bits_per_block_bind_group =
            Arc::new(gpu.device.create_bind_group(&BindGroupDescriptor {
                layout: &bits_per_block_bind_group_layout,
                entries: &[BindGroupEntry {
                    binding: 0,
                    resource: bits_per_block_buffer.as_entire_binding(),
                }],
                label: Some("Bits per Block Group"),
            }));
        let pan_zoom_buffer = gpu.device.create_buffer_init(&BufferInitDescriptor {
            label: Some("Pan Zoom Buffer"),
            contents: bytes_of(&PanZoomUniform::default()),
            usage: BufferUsages::COPY_DST | BufferUsages::UNIFORM,
        });
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
        let block_index_bind_group_layout =
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
                    label: Some("Block Index Bind Group Layout"),
                });
        let texture_bind_group_layout =
            gpu.device
                .create_bind_group_layout(&BindGroupLayoutDescriptor {
                    entries: &[BindGroupLayoutEntry {
                        binding: 0,
                        visibility: ShaderStages::FRAGMENT,
                        ty: BindingType::Texture {
                            sample_type: TextureSampleType::Uint,
                            view_dimension: TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    }],
                    label: Some("Texture Bind Group Layout"),
                });
        let pipeline_layout_desc = PipelineLayoutDescriptor {
            label: Some("Render Pipeline Layout"),
            bind_group_layouts: &[
                &bits_per_block_bind_group_layout,
                &pan_zoom_bind_group_layout,
                &block_index_bind_group_layout,
                &texture_bind_group_layout,
            ],
            push_constant_ranges: &[],
        };
        let render_pipeline_layout = gpu.device.create_pipeline_layout(&pipeline_layout_desc);
        let vertex_state = VertexState {
            module: &shader_module,
            entry_point: "vs_main",
            buffers: &[Instance::desc()],
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
        let num_blocks = 2usize.pow(16 - bits_per_block).pow(2);
        let mut blocks = Vec::with_capacity(num_blocks);
        for _ in 0..num_blocks {
            blocks.push(None);
        }
        Self {
            render_pipeline,
            pan_zoom_buffer,
            pan_zoom_bind_group,
            blocks,
            texture_bind_group_layout,
            bits_per_block_bind_group,
            bits_per_block_bind_group_layout,
            bits_per_block,
            next_to_clear: 0,
        }
    }
}

#[tracing::instrument(skip_all)]
async fn file_reader(path: impl AsRef<Path>, instance_tx: UnboundedSender<Instance>) {
    let file = File::open(&path).await.unwrap();
    let mut buf_reader = BufReader::new(file);
    let nets = range_from_path(path).iter().collect_vec();
    let instances = nets.iter().flat_map(Ipv4Net::hosts).map(Instance::from);
    let poll_dur = Duration::from_millis(10);
    for mut instance in instances {
        let val = read_f32_wait(&mut buf_reader, poll_dur).await.unwrap();
        if val >= 0. {
            instance.time = (val / 0.5 * 255.).clamp(0., 255.) as u32;
            instance_tx.send(instance).unwrap();
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

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Instance {
    pub address: u32,
    pub time: u32,
}
impl Instance {
    const ATTRS: [VertexAttribute; 2] = vertex_attr_array![0 => Uint32, 1 => Uint32];
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
            time: u32::from_be_bytes([0, 0, 0, 0]),
        }
    }
}

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

pub struct Block {
    texture: Texture,
    texture_bind_group: BindGroup,
    render_pipeline: RenderPipeline,
    instance_buffers: BufferVec<Instance>,
    block_index_bind_group: BindGroup,
}
impl Block {
    pub fn new(
        device: &Device,
        index: u32,
        texture_bind_group_layout: &BindGroupLayout,
        bits_per_block_bind_group_layout: &BindGroupLayout,
        side_length: u32,
    ) -> Self {
        let num_slots = side_length.pow(2);
        let max_buffer_size =
            std::mem::size_of::<Instance>() as BufferAddress * num_slots as BufferAddress;
        let block_index_buffer = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("Block Index Buffer"),
            contents: bytes_of(&index),
            usage: BufferUsages::UNIFORM,
        });
        let instance_buffers = BufferVec::new(max_buffer_size);
        let texture_format = TextureFormat::R8Uint;
        let texture_desc = TextureDescriptor {
            label: Some("Block Texture"),
            size: Extent3d {
                width: side_length,
                height: side_length,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: texture_format,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[texture_format],
        };
        let texture = device.create_texture(&texture_desc);
        let shader_module = device.create_shader_module(include_wgsl!("shader.wgsl"));
        let block_index_bind_group_layout =
            device.create_bind_group_layout(&BindGroupLayoutDescriptor {
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
                label: Some("Block Index Bind Group Layout"),
            });
        let block_index_bind_group = device.create_bind_group(&BindGroupDescriptor {
            layout: &block_index_bind_group_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: block_index_buffer.as_entire_binding(),
            }],
            label: Some("Block Index Bind Group"),
        });
        let pipeline_layout_desc = PipelineLayoutDescriptor {
            label: Some("Block Render Pipeline Layout"),
            bind_group_layouts: &[bits_per_block_bind_group_layout],
            push_constant_ranges: &[],
        };
        let render_pipeline_layout = device.create_pipeline_layout(&pipeline_layout_desc);
        let vertex_state = VertexState {
            module: &shader_module,
            entry_point: "vs_block",
            buffers: &[Instance::desc()],
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
            entry_point: "fs_block",
            targets: &[Some(ColorTargetState {
                format: texture_format,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
        };
        let multisample_state = MultisampleState {
            count: 1,
            mask: !0,
            alpha_to_coverage_enabled: false,
        };
        let render_pipeline_desc = RenderPipelineDescriptor {
            label: Some("Block Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: vertex_state,
            fragment: Some(fragment_state),
            primitive: primitive_state,
            depth_stencil: None,
            multisample: multisample_state,
            multiview: None,
        };
        let render_pipeline = device.create_render_pipeline(&render_pipeline_desc);
        let texture_bind_group = device.create_bind_group(&BindGroupDescriptor {
            layout: texture_bind_group_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: BindingResource::TextureView(
                    &texture.create_view(&TextureViewDescriptor::default()),
                ),
            }],
            label: Some("Texture Bind Group"),
        });
        Self {
            texture,
            render_pipeline,
            instance_buffers,
            block_index_bind_group,
            texture_bind_group,
        }
    }
    pub fn render(&mut self, encoder: &mut CommandEncoder, pan_zoom_bind_group: &BindGroup) {
        let view = self.texture.create_view(&TextureViewDescriptor::default());
        let render_pass_desc = RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(Color {
                        r: 0.,
                        g: 0.,
                        b: 0.,
                        a: 0.,
                    }),
                    store: true,
                },
            })],
            depth_stencil_attachment: None,
        };
        {
            let mut render_pass = encoder.begin_render_pass(&render_pass_desc);
            render_pass.set_pipeline(&self.render_pipeline);
            render_pass.set_bind_group(0, pan_zoom_bind_group, &[]);
            for (buffer, num_occupied) in &self.instance_buffers {
                render_pass.set_vertex_buffer(0, buffer.slice(..));
                render_pass.draw(0..6, 0..*num_occupied as _);
            }
        }
    }
}
