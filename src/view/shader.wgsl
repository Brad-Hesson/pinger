struct InstanceInput {
    @location(1) address: u32,
    @location(2) color: u32
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(1) color: vec4<f32>
}

struct PanZoomUniform {
    pan: vec2<f32>,
    zoom: vec2<f32>
}
// @group(0) @binding(0)
// var<uniform> pan_zoom: PanZoomUniform;

@vertex
fn vs_main(
    @location(0) vertex: vec2<f32>,
    instance: InstanceInput
) -> VertexOutput {
    var vertex = vertex;
    vertex += addr_to_coords(instance.address);
    // vertex += pan_zoom.pan;
    // vertex *= pan_zoom.zoom;
    var out: VertexOutput;
    out.color = color_from_u32(instance.color);
    out.clip_position = vec4<f32>(vertex, 1., 1.);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}

fn addr_to_coords(d: u32) -> vec2<f32> {
    var out = vec2<u32>(0u, 0u);
    var d = d;
    for (var s: u32 = 1u ; s < (1u << 16u); s <<= 1u) {
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
    return vec2<f32>(out) * 2. / f32(1u << 16u) - 1.;
}

fn color_from_u32(color: u32) -> vec4<f32> {
    return vec4<f32>(
        f32((color >> 24u) & 0xFFu),
        f32((color >> 16u) & 0xFFu),
        f32((color >> 8u) & 0xFFu),
        f32((color >> 0u) & 0xFFu),
    ) / 255.;
}