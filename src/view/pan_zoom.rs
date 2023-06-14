use tokio::sync::watch::Receiver;
use wgpu::{util::*, *};
use winit::{
    dpi::PhysicalSize,
    event::{ElementState, Event, MouseButton, MouseScrollDelta},
};

use crate::{gpu, view::hilbert_decode};

pub struct PanZoomState {
    pub uniform: PanZoomUniform,
    zoom: f32,
    aspect: f32,
    mouse_down: bool,
    last_position: Option<(f64, f64)>,
    modified: bool,
    addr_rx: Receiver<u32>,
    follow_mode: bool,
    pub buffer: Buffer,
    pub bind_group: BindGroup,
    pub bind_group_layout: BindGroupLayout,
}
impl PanZoomState {
    pub fn new(gpu: &gpu::GpuState, addr_rx: Receiver<u32>) -> Self {
        let pan_zoom_uniform = PanZoomUniform::default();
        let pan_zoom_buffer = gpu.device.create_buffer_init(&BufferInitDescriptor {
            label: Some("Camera Buffer"),
            contents: bytemuck::cast_slice(&[pan_zoom_uniform]),
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
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
        let aspect = gpu.surface_config.height as f32 / gpu.surface_config.width as f32;
        Self {
            uniform: pan_zoom_uniform,
            buffer: pan_zoom_buffer,
            bind_group: pan_zoom_bind_group,
            mouse_down: false,
            last_position: None,
            bind_group_layout: pan_zoom_bind_group_layout,
            zoom: 1.,
            aspect,
            modified: true,
            addr_rx,
            follow_mode: false,
        }
    }
    fn update_aspect(&mut self, new_size: &PhysicalSize<u32>) {
        self.aspect = new_size.height as f32 / new_size.width as f32;
        self.update_scale();
    }
    fn update_scale(&mut self) {
        if self.aspect >= 1. {
            self.uniform.scale = [self.zoom, self.zoom / self.aspect];
        } else {
            self.uniform.scale = [self.zoom * self.aspect, self.zoom];
        }
        self.modified = true;
    }
    pub fn pan_to(&mut self, x: u32, y: u32) {
        self.uniform.pan[0] = 1. - x as f32 / 2f32.powf(16.) * 2.;
        self.uniform.pan[1] = 1. - y as f32 / 2f32.powf(16.) * 2.;
    }
    // pub fn handle_zoom(&mut self, delta: f32, center: Option<[f64; 2]>) {
    //     let last_zoom = self.zoom;
    //     self.zoom = (last_zoom * 1.1f32.powf(delta)).max(1.);
    //     let factor = self.zoom / last_zoom;
    //     self.update_scale();
    //     if let Some([x, y]) = center {
    //         if !self.follow_mode {
    //             let dx = (x / gpu.surface_config.width as f64 * 2. - 1.)
    //                 / self.uniform.scale[0] as f64
    //                 * (factor - 1.) as f64;
    //             self.uniform.pan[0] -= dx as f32;
    //             let dx = (y / gpu.surface_config.height as f64 * 2. - 1.)
    //                 / self.uniform.scale[1] as f64
    //                 * (factor - 1.) as f64;
    //             self.uniform.pan[1] += dx as f32;
    //         }
    //     }
    // }
    pub fn handle_event(&mut self, gpu: &gpu::GpuState, event: &Event<()>) {
        if self.follow_mode && self.addr_rx.has_changed().unwrap_or(false) {
            let addr = *self.addr_rx.borrow_and_update();
            let [x, y] = hilbert_decode(addr, 32);
            self.pan_to(x, y);
            self.modified = true;
        }
        use winit::event::VirtualKeyCode::*;
        use winit::event::WindowEvent::*;
        let winit::event::Event::WindowEvent { event, .. } = event else {
            return
        };
        match event {
            KeyboardInput { input, .. } if input.virtual_keycode == Some(Space) => {
                self.follow_mode ^= input.state == ElementState::Pressed;
                if self.follow_mode {
                    let addr = *self.addr_rx.borrow_and_update();
                    let [x, y] = hilbert_decode(addr, 32);
                    self.pan_to(x, y);
                    self.modified = true;
                }
            }
            ScaleFactorChanged { new_inner_size, .. } => self.update_aspect(new_inner_size),
            Resized(physical_size) => self.update_aspect(physical_size),
            CursorMoved { position, .. } => {
                if let Some((last_x, last_y)) = self.last_position {
                    if self.mouse_down && !self.follow_mode {
                        let dx = (position.x - last_x)
                            / gpu.surface_config.width as f64
                            / self.uniform.scale[0] as f64
                            * 2.;
                        let dy = (position.y - last_y)
                            / gpu.surface_config.height as f64
                            / self.uniform.scale[1] as f64
                            * 2.;
                        self.uniform.pan[0] += dx as f32;
                        self.uniform.pan[1] -= dy as f32;
                        self.modified = true;
                    }
                }
                self.last_position = Some((position.x, position.y))
            }
            MouseWheel {
                delta: MouseScrollDelta::LineDelta(_, y),
                ..
            } => {
                let t_zoom = self.zoom;
                self.zoom = (t_zoom * 1.1f32.powf(*y)).max(1.);
                let factor = self.zoom / t_zoom;
                self.update_scale();
                if let Some((last_x, last_y)) = self.last_position {
                    if !self.follow_mode {
                        let dx = (last_x / gpu.surface_config.width as f64 * 2. - 1.)
                            / self.uniform.scale[0] as f64
                            * (factor - 1.) as f64;
                        self.uniform.pan[0] -= dx as f32;
                        let dx = (last_y / gpu.surface_config.height as f64 * 2. - 1.)
                            / self.uniform.scale[1] as f64
                            * (factor - 1.) as f64;
                        self.uniform.pan[1] += dx as f32;
                    }
                }
            }
            MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => self.mouse_down = true,
            MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => self.mouse_down = false,
            _ => {}
        }
    }
    pub fn prepare(&mut self, queue: &Queue) {
        if !self.modified {
            return;
        }
        self.modified = false;
        queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(&[self.uniform]));
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
