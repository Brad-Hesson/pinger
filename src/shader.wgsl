struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(1) uv: vec2<f32>
}

struct PanZoomUniform {
    pan: vec2<f32>,
    zoom: vec2<f32>
}

struct BlockVertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(1) texel: u32
}

struct Instance {
    @location(0) address: u32,
    @location(1) texel: u32
}

@group(0) @binding(0)
var<uniform> bits_per_block: u32;

@group(1) @binding(0)
var<uniform> pan_zoom: PanZoomUniform;

@group(2) @binding(0)
var<uniform> block_index: u32;

@group(3) @binding(0)
var texture: texture_2d<u32>;

fn total_width() -> u32 {return 1u << 16u;}
fn block_width() -> u32 {return 1u << bits_per_block;}
fn block_bits() -> u32 {return 16u - bits_per_block;}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var coords = vec2<f32>(addr_to_coords(block_index, block_bits()));
    coords *= 2. / f32(1u << block_bits());
    coords -= 1.;
    var vertex = vertex_from_index(vertex_index);
    vertex /= f32(total_width() / block_width());
    vertex += coords;
    vertex += f32(block_width()) / f32(total_width());
    vertex += pan_zoom.pan;
    vertex *= pan_zoom.zoom;
    var out: VertexOutput;
    out.clip_position = vec4<f32>(vertex, 1., 1.);
    out.uv = uv_from_index(vertex_index);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let texture_coords = vec2<i32>(uv * f32(block_width()));
    let color = textureLoad(texture, texture_coords, 0).x;
    if color == 0u {
        return vec4<f32>(0.);
    }
    return vec4<f32>(
        f32(color) / 255.,
        f32(255u - color) / 255.,
        f32(255u - color) / 255.,
        1.
    );
}

@vertex
fn vs_block(instance: Instance, @builtin(vertex_index) vertex_index: u32) -> BlockVertexOutput {
    var coords_u = addr_to_coords(instance.address, 16u) % block_width();
    let coords = (vec2<f32>(coords_u) + 0.5) / f32(block_width()) * 2. - 1.;
    var vertex = vertex_from_index(vertex_index);
    vertex /= f32(total_width());
    vertex += coords;
    var out: BlockVertexOutput;
    out.clip_position = vec4<f32>(vertex, 1., 1.);
    out.texel = instance.texel;
    return out;
}

@fragment
fn fs_block(in: BlockVertexOutput) -> @location(0) u32 {
    return in.texel;
}

fn addr_to_coords(d: u32, bits: u32) -> vec2<u32> {
    var out = vec2<u32>(0u, 0u);
    var d = d;
    for (var s: u32 = 1u ; s < (1u << bits); s <<= 1u) {
        var r: vec2<u32>;
        r.x = 1u & (d / 2u);
        r.y = 1u & (d ^ r.x);
        if r.y == 0u {
            if r.x == 1u {
                out = s - 1u - out;
            }
            let tmp = out.x;
            out.x = out.y;
            out.y = tmp;
        }
        out |= s * r;
        d >>= 2u;
    }
    return out;
}

fn vertex_from_index(index: u32) -> vec2<f32> {
    switch index {
        case 0u: {return vec2<f32>(-1., -1.);}
        case 1u, 3u: {return vec2<f32>(-1., 1.);}
        case 2u, 4u: {return vec2<f32>(1., -1.);}
        case 5u: {return vec2<f32>(1., 1.);}
        default: {return vec2<f32>(0., 0.);}
    }
}
fn uv_from_index(index: u32) -> vec2<f32> {
    switch index {
        case 0u: {return vec2<f32>(0., 1.);}
        case 1u, 3u: {return vec2<f32>(0., 0.);}
        case 2u, 4u: {return vec2<f32>(1., 1.);}
        case 5u: {return vec2<f32>(1., 0.);}
        default: {return vec2<f32>(0., 0.);}
    }
}