use wgpu::util::DeviceExt;
use winit::event::{ElementState, Event, MouseButton, MouseScrollDelta, WindowEvent};

use super::renderer::DeviceState;

pub struct PanZoomState {
    pub uniform: PanZoomUniform,
    pub buffer: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,
    pub bind_group_layout: wgpu::BindGroupLayout,
    mouse_down: bool,
    last_position: Option<(f64, f64)>,
}
impl PanZoomState {
    pub fn new(rend: &DeviceState) -> Self {
        let pan_zoom_uniform = PanZoomUniform::default();
        let pan_zoom_buffer = rend
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Camera Buffer"),
                contents: bytemuck::cast_slice(&[pan_zoom_uniform]),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });

        let pan_zoom_bind_group_layout =
            rend.device
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

        let pan_zoom_bind_group = rend.device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &pan_zoom_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: pan_zoom_buffer.as_entire_binding(),
            }],
            label: Some("Pan Zoom Bind Group"),
        });
        Self {
            uniform: pan_zoom_uniform,
            buffer: pan_zoom_buffer,
            bind_group: pan_zoom_bind_group,
            mouse_down: false,
            last_position: None,
            bind_group_layout: pan_zoom_bind_group_layout,
        }
    }
    pub fn update(&mut self, rend: &DeviceState, event: &Event<()>) {
        let winit::event::Event::WindowEvent { event, .. } = event else {
            return
        };
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                if let Some((last_x, last_y)) = self.last_position {
                    if self.mouse_down {
                        let dx = (position.x - last_x)
                            / rend.size.width as f64
                            / self.uniform.zoom[0] as f64
                            * 2.;
                        let dy = (position.y - last_y)
                            / rend.size.height as f64
                            / self.uniform.zoom[1] as f64
                            * 2.;
                        self.uniform.pan[0] += dx as f32;
                        self.uniform.pan[1] -= dy as f32;
                    }
                }
                self.last_position = Some((position.x, position.y))
            }
            WindowEvent::MouseWheel {
                delta: MouseScrollDelta::LineDelta(_, y),
                ..
            } => {
                self.uniform.zoom[0] *= 1.1f32.powf(*y);
                self.uniform.zoom[1] *= 1.1f32.powf(*y);
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
            _ => {}
        }
    }
    pub fn update_buffer(&self, renderer: &DeviceState) {
        renderer
            .queue
            .write_buffer(&self.buffer, 0, bytemuck::cast_slice(&[self.uniform]));
    }
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PanZoomUniform {
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
