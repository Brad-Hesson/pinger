use std::net::Ipv4Addr;

use ipnet::Ipv4Net;
use iprange::IpRange;
use itertools::Itertools;
use piston_window::{
    color,
    math::{self, identity},
    triangulation::{rect_tri_list_xy, tx, ty},
    DrawState, Graphics,
};

pub struct PingMap {
    white_vertices: Vec<[f32; 2]>,
    white_buffer: Vec<[f32; 2]>,
    white_indices: Vec<usize>,
}
impl PingMap {
    pub fn new(bools: Vec<bool>, range: IpRange<Ipv4Net>) -> Self {
        let white_vertices = range
            .iter()
            .flat_map(|net| net.hosts())
            .map(hilbert_from_addr)
            .map(|[x, y]| {
                [
                    x as f64 / 65536. * 2. - 1.,
                    1. - y as f64 / 65536. * 2.,
                    2. / 65536.,
                    2. / 65536.,
                ]
            })
            .zip(bools)
            .filter_map(|(v, b)| b.then_some(v))
            .flat_map(|v| rect_tri_list_xy(identity(), v))
            .collect::<Vec<_>>();
        dbg!(white_vertices.len());
        let white_indices = (0..)
            .map(|v| v * 1023)
            .take_while(|v| *v < white_vertices.len())
            .chain(Some(white_vertices.len()))
            .collect::<Vec<_>>();
        Self {
            white_buffer: vec![[0., 0.]; white_vertices.len()],
            white_vertices,
            white_indices,
        }
    }
    pub fn draw<G: Graphics>(&mut self, transform: math::Matrix2d, g: &mut G) {
        // Load the buffer with the tranformed vertices
        for (i, [x, y]) in self.white_vertices.iter().enumerate() {
            self.white_buffer[i] = [
                tx(transform, *x as f64, *y as f64),
                ty(transform, *x as f64, *y as f64),
            ];
        }
        // In the draw closure, simply iterate through the chuncks
        // and send them to the gpu
        let f = |send: &mut dyn FnMut(&[[f32; 2]])| {
            for (a, b) in self.white_indices.iter().tuple_windows() {
                send(&self.white_buffer[*a..*b]);
            }
        };
        g.tri_list(&DrawState::default(), &color::WHITE, f);
    }
}

fn hilbert_from_addr(addr: Ipv4Addr) -> [u32; 2] {
    let mut bits = u32::from_be_bytes(addr.octets());
    let mut x_pos = 0u32;
    let mut y_pos = 0u32;
    for i in 0..16 {
        let a = bits & 1;
        bits >>= 1;
        let b = bits & 1;
        bits >>= 1;
        let x = b;
        let y = a ^ b;
        if y == 0 && i > 0 {
            if x == 1 {
                x_pos = (1 << i) - 1 - x_pos;
                y_pos = (1 << i) - 1 - y_pos;
            }
            std::mem::swap(&mut x_pos, &mut y_pos);
        }
        x_pos |= x << i;
        y_pos |= y << i;
    }
    [x_pos, y_pos]
}
