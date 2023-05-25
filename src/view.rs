use std::{
    fs::File,
    io::{BufReader, Read},
    net::Ipv4Addr,
    path::Path,
};

use ipnet::Ipv4Net;
use iprange::IpRange;
use itertools::Itertools;
use piston_window::{
    math::identity,
    triangulation::{rect_tri_list_xy, tx, ty},
    *,
};

use self::pan_zoom::PanZoom;

mod pan_zoom;

pub async fn main(args: Args) {
    let mut file = BufReader::new(File::open(&args.filepath).unwrap());
    let range = range_from_path(args.filepath);
    let bools = read_file(&mut file);
    let mut ping_map = PingMap::new(bools, range);
    let mut window: PistonWindow = WindowSettings::new("Pinger Viewer", [720, 480])
        .exit_on_esc(true)
        .build()
        .unwrap();
    let mut pan_zoom = PanZoom::new(1.1, ..);
    while let Some(event) = window.next() {
        pan_zoom.update(&event);
        window.draw_2d(&event, |mut ctx, g, _| {
            pan_zoom.apply_transform(&mut ctx);
            clear(color::GRAY, g);
            ping_map.draw(ctx.transform, g);
        });
    }
}

struct PingMap {
    white_vertices: Vec<[f32; 2]>,
    white_t_buffer: Vec<[f32; 2]>,
    white_slice_indices: Vec<usize>,
}
impl PingMap {
    fn new(bools: Vec<bool>, range: IpRange<Ipv4Net>) -> Self {
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
        let white_slice_indices = (0..)
            .map(|v| v * 1023)
            .take_while(|v| *v < white_vertices.len())
            .chain(Some(white_vertices.len()))
            .collect::<Vec<_>>();
        Self {
            white_t_buffer: vec![[0., 0.]; white_vertices.len()],
            white_vertices,
            white_slice_indices,
        }
    }
    fn draw<G: Graphics>(&mut self, transform: math::Matrix2d, g: &mut G) {
        for (i, [x, y]) in self.white_vertices.iter().enumerate() {
            self.white_t_buffer[i] = [
                tx(transform, *x as f64, *y as f64),
                ty(transform, *x as f64, *y as f64),
            ];
        }
        let f = |send: &mut dyn FnMut(&[[f32; 2]])| {
            for (a, b) in self.white_slice_indices.iter().tuple_windows() {
                send(&self.white_t_buffer[*a..*b]);
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

fn center_rect<P, G>(color: types::Color, pos: P, size: P, transform: math::Matrix2d, g: &mut G)
where
    P: Into<[f64; 2]>,
    G: Graphics,
{
    let pos = pos.into();
    let size = size.into();
    let r = [
        pos[0] - size[0] / 2.,
        pos[1] - size[1] / 2.,
        size[0],
        size[1],
    ];
    rectangle(color, r, transform, g);
}

#[derive(Debug, clap::Args)]
pub struct Args {
    filepath: String,
}
