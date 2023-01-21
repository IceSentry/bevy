// This shader computes the chromatic aberration effect

// Since post process is a fullscreen effect, we use the fullscreen vertex stage from bevy
// This will render a single fullscreen triangle.
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

// This function will give you the tex_coord of the screen texture for the current fragment position
fn get_screen_coord(in: FullscreenVertexOutput) -> vec2<f32> {
    let resolution = vec2<f32>(textureDimensions(screen_texture));
    let frag_coord = in.position.xy;
    let inverse_screen_size = 1.0 / resolution.xy;
    return in.position.xy * inverse_screen_size;
}

fn coords_to_viewport_uv(position: vec2<f32>, viewport: vec4<f32>) -> vec2<f32> {
    return (position - viewport.xy) / viewport.zw;
}

fn prepass_depth(frag_coord: vec2<f32>) -> f32 {
    let depth_sample = textureLoad(depth_prepass_texture, vec2<i32>(frag_coord), 0);
    return depth_sample;
}

fn prepass_normal(frag_coord: vec2<f32>) -> vec3<f32> {
    let normal_sample = textureLoad(normal_prepass_texture, vec2<i32>(frag_coord), 0);
    // return normal_sample.xyz;
    return normal_sample.xyz * 2.0 - vec3(1.0);
}

fn hash33(p3: vec3<f32>) -> vec3<f32> {
    var out = fract(p3 * vec3(0.1031, 0.1030, 0.0973));
    out += dot(out, out.yxz + 33.33);
    return fract((out.xxy + out.yxx) * out.zyx);
}

var<private> sky_color: vec3<f32> = vec3<f32>(0.0, 0.0, 0.0);

struct ReflectionSampleData {
    depth: f32,
	color: vec4<f32>,
	point_along_ray_dist_to_camera: f32,
	uv: vec2<f32>,
};

fn calculateReflectionSampleData(world_pos: vec4<f32>, reflection_dir: vec3<f32>, dist_along_ray: f32) -> ReflectionSampleData {
    let point_along_ray_in_world_space = world_pos.xyz + reflection_dir * dist_along_ray;
    let point_along_ray_in_screen_space = view.projection * vec4(point_along_ray_in_world_space, 1.0);

    var uv_proj = point_along_ray_in_screen_space / point_along_ray_in_screen_space.w;
    uv_proj = uv_proj * 0.5 + 0.5;
    let uv = uv_proj.xy;

    let depth = prepass_depth(point_along_ray_in_screen_space.xy);
    return ReflectionSampleData(
        depth,
        textureSample(screen_texture, texture_sampler, uv),
        distance(point_along_ray_in_world_space.xyz, view.world_position),
        uv
    );
}

// Call this to change the colour of the material without reflections into a colour that includes reflections.
// Whether reflection is visible at this pixel, depends on the reflectionStrength, which is read from a texture
// that defines the material of this object.
fn apply_reflection(
    original_position_in_proj_space: vec4<f32>,
    world_pos: vec4<f32>,
    normal: vec3<f32>,
    color_before: vec4<f32>,
    strength: f32
) -> vec3<f32> {
	// We're going to march over the reflection ray, but we're going to do so in screen space.
	// As soon as we find a pixel that's on the other side of the ray, we've found
    // a hit and know what we're reflecting.
    // This implementation assumes that depth is stored in the alpha channel
    // of the renderTexture that also contains the colours.

    // First calculate the reflectionRay.
    let view_dir_to_pixel = world_pos.xyz - view.world_position;
    var reflection_dir = normalize(reflect(view_dir_to_pixel, normalize(normal)));

	// Randomise the reflection direction based on the roughtness of the surface.
	// The magic number multiplication is based on what looks good in combination with
    // the usage of roughness below for blending with the blurred texture.
    let roughness = 0.5;
    reflection_dir += (hash33(world_pos.xyz * 10.0) - vec3(0.5)) * roughness * 0.02;

    var reflection_color = sky_color;
    var prev_point_along_ray_dist_to_camera = distance(world_pos.xyz, view.world_position);
    var prev_dist_along_ray = 0.0;
    var curr_dist_along_ray = 0.0;
    var curr_step_dist = 0.05;
    var curr_uv = vec2(0.0);
    var collision_found = false;

    for (var i = 1; i <= 20; i++) {
        prev_dist_along_ray = curr_dist_along_ray;

        // We take steps of increasing distance, so that we get more precision near the surface and less in the distance.
		// This works nicely with the characters that are usually standing directly on the reflection surface
        // and are thus near and reflected with most precision. Also, we fade out reflections in the distance
        // and blur them in the distance, so loss of precision far away is less problematic.
        curr_dist_along_ray += curr_step_dist;

        let sample_data = calculateReflectionSampleData(world_pos, reflection_dir, curr_dist_along_ray);
        if i == 2 {
            return vec3(sample_data.depth);
            // return reflection_dir;
            // return vec3(0.01 / sample_data.depth);
                        // if sample_data.depth < 0.0000001 {
                        //     return vec3(0.0, 1.0, 0.0);
                        // } else {
                        //     return vec3(0.0, 0.0, 1.0);
                        // }
            // if sample_data.depth <= sample_data.point_along_ray_dist_to_camera {
            //     return vec3(1.0, 0.0, 0.0);
            //     // return vec3(sample_data.color.rgb);
            // }
        }

        // if sample_data.depth > prev_point_along_ray_dist_to_camera - curr_step_dist {
        //     return vec3(0.0, 1.0, 0.0);
        // }
        if sample_data.depth <= sample_data.point_along_ray_dist_to_camera && sample_data.depth > prev_point_along_ray_dist_to_camera - curr_step_dist {
            reflection_color = sample_data.color.rgb;
            curr_uv = sample_data.uv;
            collision_found = true;
            return vec3(1.0, 0.0, 1.0);
            // break;
        }
        prev_point_along_ray_dist_to_camera = sample_data.point_along_ray_dist_to_camera;
        curr_step_dist += 0.05;
    }
    if collision_found {
        return vec3(1.0, 0.0, 0.0);
    }

    return mix(color_before.rgb, reflection_color.rgb, strength);
    // return vec3(1.0);
}

fn reconstruct_view_space_position(depth: f32, uv: vec2<f32>) -> vec3<f32> {
    let clip_xy = vec2(uv.x * 2.0 - 1.0, 1.0 - 2.0 * uv.y);
    let t = view.inverse_projection * vec4(clip_xy, depth, 1.0);
    let view_xyz = t / t.w;
    return (view.inverse_view * view_xyz).xyz;
}

fn linearize_depth(d: f32, zNear: f32, zFar: f32) -> f32 {
    return zNear * zFar / (zFar + d * (zNear - zFar));
}

// "scale_factor" is "v_fov_scale".
fn calculate_view_position(tex_coord: vec2<f32>, depth: f32, scale_factor: vec2<f32>) -> vec3<f32> {
    // No need to multiply by two, because we already baked that into "v_tan_fov.xy".
    let half_ndc_position = vec2(0.5) - tex_coord;
    // "-depth" because in OpenGL the camera is staring down the -z axis (and we're storing the unsigned depth).
    let view_space_position = vec3(half_ndc_position * scale_factor.xy * depth, depth);
    return view_space_position;
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    // let uv = get_screen_coord(in);
    let uv = coords_to_viewport_uv(in.position.xy, view.viewport);
    let color = textureSample(screen_texture, texture_sampler, uv);

    let depth = prepass_depth(in.position.xy);
    let normal = prepass_normal(in.position.xy);

    // let x = uv.x * 2.0 - 1.0;
    // let y = (1.0 - uv.y) * 2.0 - 1.0;
    // let projected_pos = vec4(x, y, (0.001 / depth), 1.0);
    // let pos_vs = projected_pos * view.inverse_view;
    // let pos_ws = pos_vs.xyz / pos_vs.w;

    // let clip_space_pos = vec4(uv * 2.0 - 1.0, (0.001 / depth), 1.0);
    // var view_space_pos = view.inverse_view * clip_space_pos;
    // view_space_pos /= view_space_pos.w;
    // let world_space_pos = view.inverse_view * view_space_pos;

    let pixel_position = reconstruct_view_space_position(depth, in.position.xy);
    let fov = 3.14159 / 4.0;
    var fov_scale = vec2(tan(fov / 2.0), tan(fov / 2.0));
    fov_scale *= 2.0;

    let pos = calculate_view_position(uv, depth, fov_scale);

    // // Build the prepass depth into a point in ndc space
    // // let depth = prepass_depth(in.position.xy);
    // let frag_uv = coords_to_viewport_uv(in.position.xy, view.viewport);
    // let prepass_ndc_xy = vec2(uv.x * 2.0 - 1.0, (1.0 - uv.y) * 2.0 - 1.0);
    // // Transform the point into clip space
    // let prepass_clip = vec4(prepass_ndc_xy, depth, 1.0);
    // // Transform the point into view space, note the perspective divide
    // let view_undiv = view.inverse_view_proj * prepass_clip;
    // let prepass_view = view_undiv.xyz / view_undiv.w;
    // // Transform the point into world space to find the distance to the camera
    // // let prepass_world = prepass_view * view.inverse_view;

    let reflected = normalize(reflect(normalize(pixel_position.xyz), normalize(normal)));

    // let reflection = apply_reflection(view.projection * in.position, world_space_pos, normal, color, 0.5);
    return vec4(vec3(pixel_position.xyz), 1.0);
    // return vec4(prepass_ndc_xy, 1.0, 1.0);
    // return vec4(reflection, 1.0);
    // return vec4(reflected, 1.0);
    // return vec4(vec3(view_space_pos.xyz), 1.0);
    // return color;
    // return vec4(vec3(0.001 / depth), 1.0);
    // return vec4(normal, 1.0);
}

