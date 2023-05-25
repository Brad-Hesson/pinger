use std::{
    fs::File,
    io::{BufReader, Read},
    path::Path,
};

use ipnet::Ipv4Net;
use iprange::IpRange;
use piston_window::*;

use self::pan_zoom::PanZoom;

mod pan_zoom;
mod ping_map;

pub async fn main(args: Args) {
    let mut file = BufReader::new(File::open(&args.filepath).unwrap());
    let range = range_from_path(args.filepath);
    let bools = read_file(&mut file);
    let mut ping_map = ping_map::PingMap::new(bools, range);
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
            rectangle(
                color::BLACK,
                [-1., -1., 2. + 2. / 65536., 2. + 2. / 65536.],
                ctx.transform,
                g,
            );
            ping_map.draw(ctx.transform, g);
        });
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

fn read_file(file: &mut BufReader<File>) -> Vec<bool> {
    let mut bools = vec![];
    let mut buf = [0u8; 4];
    while file.read_exact(&mut buf).is_ok() {
        let val = f32::from_be_bytes(buf);
        bools.push(val >= 0.);
    }
    bools
}

#[derive(Debug, clap::Args)]
pub struct Args {
    filepath: String,
}
