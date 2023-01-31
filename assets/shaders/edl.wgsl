#import bevy_pbr::mesh_types
#import bevy_pbr::mesh_view_bindings

@group(1) @binding(0)
var<uniform> color: vec4<f32>;

fn prepass_depth(frag_coord: vec2<f32>, si: u32) -> f32 {
    let depth_sample = textureLoad(depth_prepass_texture, vec2<i32>(frag_coord), i32(si));
    return 0.001 / depth_sample;
}

@fragment
fn fragment(
    @builtin(position) frag_coord: vec4<f32>,
    @builtin(sample_index) si: u32,
    #import bevy_pbr::mesh_vertex_output
) -> @location(0) vec4<f32> {
    var depth = prepass_depth(frag_coord.xy, si);

    var response = 0.0;
    response += max(0.0, depth - prepass_depth(vec2(frag_coord.x + 1.0, frag_coord.y), si));
    response += max(0.0, depth - prepass_depth(vec2(frag_coord.x - 1.0, frag_coord.y), si));
    response += max(0.0, depth - prepass_depth(vec2(frag_coord.x, frag_coord.y + 1.0), si));
    response += max(0.0, depth - prepass_depth(vec2(frag_coord.x, frag_coord.y - 1.0), si));
    response /= 4.0;

    var shade = exp(-response * 3000.0 * 1.0);

    // shade = clamp(shade, 0.0, 0.01);

    // return color * vec4(shade);
    // return color;
    return vec4(shade);
}
