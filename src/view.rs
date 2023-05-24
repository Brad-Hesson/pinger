use std::{
    fs::File,
    io::{BufReader, Read},
    net::Ipv4Addr,
    path::PathBuf,
    str::FromStr,
};

use ipnet::Ipv4Net;
use iprange::IpRange;
use itertools::izip;
use piston_window::*;

use self::pan_zoom::PanZoom;

mod pan_zoom;

pub async fn main(args: Args) {
    let filepath = PathBuf::from_str(&args.filepath).unwrap();
    let mut file = BufReader::new(File::open(&filepath).unwrap());
    let range = range_from_path(filepath.file_stem().unwrap().to_str().unwrap());
    let bools = read_file(&mut file);
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
            let addrs = range.into_iter().flat_map(|net| net.hosts());
            for (addr, b) in izip!(addrs, &bools) {
                let [x, y] = hilbert_from_addr(addr);
                let x = x as f64 / 65536. * 2. - 1.;
                let y = y as f64 / 65536. * 2. - 1.;
                let color = if *b {color::WHITE} else {color::BLACK};
                center_rect(color, [x, y], [2. / 65536.; 2], ctx.transform, g);
            }
        });
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

fn range_from_path(filename: &str) -> IpRange<Ipv4Net> {
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
