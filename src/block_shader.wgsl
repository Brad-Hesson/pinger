struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(1) texel: u32
}

struct Instance {
    @location(0) address: u32,
    @location(1) texel: u32
}

struct PanZoomUniform {
    pan: vec2<f32>,
    zoom: vec2<f32>
}

@group(0) @binding(0)
var<uniform> block_index: u32;

const BLOCK_BITS: u32 = 3u;
fn total_width() -> f32 {return f32(1u << 16u);}
fn block_width() -> f32 {return f32(1u << (16u - BLOCK_BITS));}

@vertex
fn vs_main(instance: Instance, @builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var coords_u = addr_to_coords(instance.address, 16u);
    coords_u = coords_u % u32(block_width());
    let coords = vec2<f32>(coords_u) / block_width() * 2. - 1.;
    var vertex = vertex_from_index(vertex_index);
    vertex /= total_width();
    vertex += coords;
    var out: VertexOutput;
    out.clip_position = vec4<f32>(vertex, 1., 1.);
    out.texel = instance.texel;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<u32> {
    return vec4<u32>(in.texel);
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
    if (bits % 2u) == 1u {
        let tmp = out.x;
        out.x = out.y;
        out.y = tmp;
    }
    return out;
}

fn color_from_u32(color: u32) -> vec4<f32> {
    return vec4<f32>(
        f32((color >> 24u) & 0xFFu),
        f32((color >> 16u) & 0xFFu),
        f32((color >> 8u) & 0xFFu),
        f32((color >> 0u) & 0xFFu),
    ) / 255.;
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
        case 0u: {return vec2<f32>(0., 0.);}
        case 1u, 3u: {return vec2<f32>(0., 1.);}
        case 2u, 4u: {return vec2<f32>(1., 0.);}
        case 5u: {return vec2<f32>(1., 1.);}
        default: {return vec2<f32>(0., 0.);}
    }
}