use std::{marker::PhantomData, slice::Iter};

use bytemuck::cast_slice;
use tracing::Level;
use wgpu::{Buffer, BufferAddress, BufferDescriptor, BufferUsages, Device, Queue};

pub struct BufferVec<T> {
    instance_buffers: Vec<(Buffer, usize)>,
    max_buffer_size: BufferAddress,
    num_slots: usize,
    _t: PhantomData<T>,
}
impl<T> BufferVec<T> {
    const DATA_SIZE: BufferAddress = std::mem::size_of::<T>() as _;
    pub fn new(max_buffer_size: BufferAddress) -> Self {
        Self {
            instance_buffers: vec![],
            max_buffer_size,
            num_slots: (max_buffer_size / Self::DATA_SIZE) as _,
            _t: PhantomData,
        }
    }
    fn push_new_buffer(&mut self, device: &Device) {
        self.instance_buffers.push((
            device.create_buffer(&BufferDescriptor {
                label: None,
                size: self.max_buffer_size,
                usage: BufferUsages::COPY_DST | BufferUsages::VERTEX,
                mapped_at_creation: false,
            }),
            0,
        ));
    }
    pub fn extend(&mut self, device: &Device, queue: &Queue, mut data: &[T])
    where
        T: bytemuck::Pod,
    {
        let span = tracing::span!(Level::TRACE, "Extend Buffers");
        let _span = span.enter();
        if self.instance_buffers.is_empty() {
            self.push_new_buffer(device);
        }
        loop {
            let (buffer, num_occupied) = self.instance_buffers.last_mut().unwrap();
            let remaining_slots = self.num_slots - *num_occupied;
            let offset = *num_occupied as BufferAddress * Self::DATA_SIZE;
            if data.len() < remaining_slots {
                queue.write_buffer(buffer, offset, cast_slice(data));
                *num_occupied += data.len();
                break;
            }
            queue.write_buffer(buffer, offset, cast_slice(&data[..remaining_slots]));
            *num_occupied += remaining_slots;
            data = &data[remaining_slots..];
            self.push_new_buffer(device);
        }
    }

    pub fn iter(&self) -> Iter<'_, (Buffer, usize)> {
        self.instance_buffers.iter()
    }

    pub fn clear(&mut self) {
        self.instance_buffers.clear()
    }
}
impl<'a, T> IntoIterator for &'a BufferVec<T> {
    type Item = &'a (Buffer, usize);

    type IntoIter = Iter<'a, (Buffer, usize)>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
