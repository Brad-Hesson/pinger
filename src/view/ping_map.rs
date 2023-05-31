use std::net::Ipv4Addr;

use tokio::sync::mpsc::UnboundedReceiver;
use wgpu::{util::DeviceExt, VertexAttribute};

use super::renderer::DeviceState;

pub struct PingMapState {
    pub instances: Vec<Instance>,
    pub indicies: Vec<u16>,
    pub instance_buffer: wgpu::Buffer,
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    pub rx: UnboundedReceiver<Instance>,
}
impl PingMapState {
    pub fn new(gpu: &DeviceState, rx: UnboundedReceiver<Instance>) -> Self {
        let instance_buffer = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Instance Buffer"),
                contents: &[],
                usage: wgpu::BufferUsages::VERTEX,
            });
        let vertex_buffer = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Vertex Buffer"),
                contents: bytemuck::cast_slice(VERTICES),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let index_buffer = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Index Buffer"),
                contents: bytemuck::cast_slice(INDICES),
                usage: wgpu::BufferUsages::INDEX,
            });

        Self {
            indicies: INDICES.into(),
            instances: vec![],
            instance_buffer,
            rx,
            vertex_buffer,
            index_buffer,
        }
    }
    pub fn update_buffer(&mut self, gpu: &DeviceState) {
        let mut updated = false;
        while let Ok(i) = self.rx.try_recv() {
            self.instances.push(i);
            updated = true;
        }
        if updated {
            let mut temp_instance_buffer =
                gpu.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("Instance Buffer"),
                        contents: bytemuck::cast_slice(&self.instances[..]),
                        usage: wgpu::BufferUsages::VERTEX,
                    });
            std::mem::swap(&mut self.instance_buffer, &mut temp_instance_buffer);
            temp_instance_buffer.destroy();
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Instance {
    hilb: u32,
}
impl Instance {
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![2 => Uint32],
        }
    }
}
impl From<Ipv4Addr> for Instance {
    fn from(addr: Ipv4Addr) -> Self {
        Self {
            hilb: u32::from_be_bytes(addr.octets()),
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    position: [f32; 3],
    uv: [f32; 2],
}
impl Vertex {
    const ATTRS: [VertexAttribute; 2] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2];
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
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
