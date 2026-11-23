// Work in byte-domain RGB as the reference compositor does.
struct Viewport { size: vec2<f32>, padding: vec2<f32> }
@group(0) @binding(0) var<uniform> viewport: Viewport;
@group(0) @binding(1) var<storage, read> columns: array<u32>;
@group(1) @binding(0) var sprite: texture_2d<f32>;

struct Instance {
    @location(0) rect: vec4<f32>,
    @location(1) shade: vec4<u32>,
    @location(2) nodes_a: vec4<u32>,
    @location(3) nodes_b: vec4<u32>,
}
struct Output {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) origin: vec2<f32>,
    @location(1) @interpolate(flat) shade: vec4<u32>,
    @location(2) @interpolate(flat) nodes_a: vec4<u32>,
    @location(3) @interpolate(flat) nodes_b: vec4<u32>,
}

@vertex
fn vs(instance: Instance, @builtin(vertex_index) vertex: u32) -> Output {
    let corners = array(vec2(0., 0.), vec2(1., 0.), vec2(0., 1.),
                        vec2(0., 1.), vec2(1., 0.), vec2(1., 1.));
    let relative = (instance.rect.xy + corners[vertex] * instance.rect.zw) / viewport.size;
    var out: Output;
    out.position = vec4(relative * vec2(2., -2.) + vec2(-1., 1.), 0., 1.);
    out.origin = instance.rect.xy;
    out.shade = instance.shade;
    out.nodes_a = instance.nodes_a;
    out.nodes_b = instance.nodes_b;
    return out;
}

fn rgb(word: u32) -> vec3<u32> {
    return vec3(word & 255u, (word >> 8u) & 255u, (word >> 16u) & 255u);
}

fn node(input: Output, index: u32) -> vec3<u32> {
    if index < 4u { return rgb(input.nodes_a[index]); }
    if index < 8u { return rgb(input.nodes_b[index - 4u]); }
    return rgb(input.shade.z);
}

@fragment
fn fs(input: Output) -> @location(0) vec4<f32> {
    let p = vec2<u32>(input.position.xy - input.origin);
    // Integer texel coordinates mean nearest sampling with no interpolation,
    // including at sprite edges and when a quad is clipped.
    let texel = vec4<u32>(round(textureLoad(sprite, vec2<i32>(p), 0) * 255.));
    if texel.a == 0u { discard; }
    var color = texel.rgb;
    switch input.shade.x {
        case 1u: { color = color * rgb(input.shade.y) / 255u; }
        case 2u: {
            let start = p.y / 20u * 3u + p.x / 39u;
            let x = p.x % 39u;
            let y = p.y % 20u;
            let tint = (node(input, start) * ((39u - x) * (20u - y))
                      + node(input, start + 1u) * (x * (20u - y))
                      + node(input, start + 3u) * ((39u - x) * y)
                      + node(input, start + 4u) * (x * y)) / 780u;
            color = color * tint / 255u;
        }
        case 3u: { color = color * rgb(columns[input.shade.y + p.x]) / 255u; }
        default: {}
    }
    return vec4<f32>(vec4(color, texel.a)) / 255.;
}
