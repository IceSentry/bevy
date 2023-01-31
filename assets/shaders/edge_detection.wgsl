#import bevy_core_pipeline::fullscreen_vertex_shader
// #import bevy_pbr::utils

struct View {
    view_proj: mat4x4<f32>,
    inverse_view_proj: mat4x4<f32>,
    view: mat4x4<f32>,
    inverse_view: mat4x4<f32>,
    projection: mat4x4<f32>,
    inverse_projection: mat4x4<f32>,
    world_position: vec3<f32>,
    // viewport(x_origin, y_origin, width, height)
    viewport: vec4<f32>,
};

struct Config {
    depth_threshold: f32,
    normal_threshold: f32,
    color_threshold: f32,
    edge_color: vec4<f32>,
    debug: f32,
    enabled: f32,
};

@group(0) @binding(0)
var screen_texture: texture_2d<f32>;
@group(0) @binding(1)
var texture_sampler: sampler;
@group(0) @binding(2)
var depth_prepass_texture: texture_depth_2d;
@group(0) @binding(3)
var normal_prepass_texture: texture_2d<f32>;
@group(0) @binding(4)
var<uniform> view: View;
@group(0) @binding(5)
var<uniform> config: Config;

fn prepass_depth(frag_coord: vec2<f32>) -> f32 {
    let depth_sample = textureLoad(depth_prepass_texture, vec2<i32>(frag_coord), 0);
    return 0.001 / depth_sample;
}

@fragment
fn fragment(
    in: FullscreenVertexOutput
) -> @location(0) vec4<f32> {
    let frag_coord = in.position.xy;

    let resolution = vec2<f32>(textureDimensions(screen_texture));
    let inverse_screen_size = 1.0 / resolution.xy;
    let uv = frag_coord * inverse_screen_size;
    let color = textureSample(screen_texture, texture_sampler, uv);

    var log_depth = prepass_depth(frag_coord);

    if config.enabled == 1.0 {
        var response = 0.0;
        response += max(0.0, log_depth - prepass_depth(vec2(frag_coord.x + 1.0, frag_coord.y)));
        response += max(0.0, log_depth - prepass_depth(vec2(frag_coord.x - 1.0, frag_coord.y)));
        response += max(0.0, log_depth - prepass_depth(vec2(frag_coord.x, frag_coord.y + 1.0)));
        response += max(0.0, log_depth - prepass_depth(vec2(frag_coord.x, frag_coord.y - 1.0)));
        response /= 4.0;

        var shade = exp(-response * 3000.0 * 1.0);

        return color * vec4(shade);
    } else {
        return color;
    }
}