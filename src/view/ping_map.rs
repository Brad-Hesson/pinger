use std::net::Ipv4Addr;

use itertools::Itertools;
use tokio::sync::mpsc::UnboundedReceiver;
use wgpu::{util::*, *};

use crate::gpu;

pub struct PingMapState {
    pub instances: Vec<Instance>,
    pub indicies: Vec<u16>,
    pub instance_buffers: Vec<(usize, Buffer)>,
    pub vertex_buffer: Buffer,
    pub index_buffer: Buffer,
    pub rx: UnboundedReceiver<Instance>,
}
impl PingMapState {
    pub fn new(gpu: &gpu::GpuState, rx: UnboundedReceiver<Instance>) -> Self {
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

        Self {
            indicies: INDICES.into(),
            instances: vec![],
            instance_buffers: vec![],
            rx,
            vertex_buffer,
            index_buffer,
        }
    }
    pub fn prepare(&mut self, device: &Device) {
        let mut updated = false;
        while let Ok(i) = self.rx.try_recv() {
            self.instances.push(i);
            updated = true;
        }
        if updated {
            let max_instance_buffer =
                device.limits().max_buffer_size as usize / std::mem::size_of::<Instance>();
            let inds = (0..)
                .map(|v| v * max_instance_buffer)
                .take_while(|v| *v < self.instances.len())
                .chain(Some(self.instances.len()))
                .tuple_windows::<(_, _)>();
            self.instance_buffers.clear();
            for (a, b) in inds {
                let buffer = device.create_buffer_init(&BufferInitDescriptor {
                    label: Some("Instance Buffer"),
                    contents: bytemuck::cast_slice(&self.instances[a..b]),
                    usage: BufferUsages::VERTEX,
                });
                self.instance_buffers.push((b - a, buffer));
            }
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
