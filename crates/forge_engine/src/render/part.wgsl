// The shader of every part (see `render/part_material.rs`).
//
// A part's colour and see-through-ness are not in its material: they are in the instance's `MeshTag`
// (red, green, blue and alpha as four bytes, sRGB). All parts of one material therefore share it, and
// Bevy draws all of them with the same mesh in a single instanced draw call, whatever their colours.
//
// What this adds to Bevy's standard PBR shading:
//  * the part's colour tints the material's colour (and colour map), its alpha scales the material's opacity;
//  * "studs" texture coordinates: the maps are projected along the part's own axes and measured in studs, so
//    a texture keeps its size on a part of any size and shows on every shape without stretching;
//  * emission in the part's own colour (Neon), normal-map and occlusion strength;
//  * glass: the surface gets more opaque at grazing angles.

#import bevy_pbr::{
    mesh_functions,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
}

#ifdef PREPASS_PIPELINE
#import bevy_pbr::{
    prepass_io::{VertexOutput, FragmentOutput},
    pbr_deferred_functions::deferred_output,
}
#else
#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
}
#endif

// Keep in step with `PartLookUniform` in part_material.rs (same fields, same order).
struct PartLook {
    // 1: draw the part; 0: draw nothing (the wireframe-only view).
    solid: f32,
    opacity: f32,
    edge_opacity: f32,
    emission_from_color: f32,
    studs_uv: f32,
    normal_strength: f32,
    occlusion_strength: f32,
    padding: f32,
    studs_per_tile: vec2<f32>,
    padding2: vec2<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100)
var<uniform> look: PartLook;

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(high, low, c <= vec3<f32>(0.04045));
}

// The four bytes of the tag, as 0..1.
fn unpack_tag(tag: u32) -> vec4<f32> {
    return vec4<f32>(
        f32(tag & 255u),
        f32((tag >> 8u) & 255u),
        f32((tag >> 16u) & 255u),
        f32((tag >> 24u) & 255u),
    ) / 255.0;
}

// Texture coordinates and a matching tangent from the part's own axes, in studs. The face is the one the surface
// normal (in the part's frame) points at the most; the maps are laid out as seen from outside that face, u to the right
// and v down, so no face is mirrored and a normal map's "up" is up on every side.
struct StudsFrame {
    uv: vec2<f32>,
    tangent: vec3<f32>,
}

fn studs_frame(world_position: vec3<f32>, world_normal: vec3<f32>, instance_index: u32) -> StudsFrame {
    let m = mesh_functions::get_world_from_local(instance_index);
    let ax = m[0].xyz;
    let ay = m[1].xyz;
    let az = m[2].xyz;
    // The part's axes as unit vectors in the world (the matrix also holds the size).
    let ux = normalize(ax);
    let uy = normalize(ay);
    let uz = normalize(az);
    let rel = world_position - m[3].xyz;
    let p = vec3<f32>(dot(rel, ux), dot(rel, uy), dot(rel, uz));
    let n = vec3<f32>(dot(world_normal, ux), dot(world_normal, uy), dot(world_normal, uz));
    let a = abs(n);
    var out: StudsFrame;
    if a.x >= a.y && a.x >= a.z {
        let s = select(-1.0, 1.0, n.x >= 0.0);
        out.uv = vec2<f32>(-s * p.z, -p.y);
        out.tangent = uz * (-s);
    } else if a.y >= a.z {
        let s = select(-1.0, 1.0, n.y >= 0.0);
        out.uv = vec2<f32>(p.x, s * p.z);
        out.tangent = ux;
    } else {
        let s = select(-1.0, 1.0, n.z >= 0.0);
        out.uv = vec2<f32>(s * p.x, -p.y);
        out.tangent = ux * s;
    }
    return out;
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    if look.solid < 0.5 {
        discard;
    }
    var vin = in;
    if look.studs_uv > 0.5 {
        let frame = studs_frame(in.world_position.xyz, normalize(in.world_normal), in.instance_index);
        vin.uv = frame.uv / look.studs_per_tile;
#ifdef VERTEX_TANGENTS
        vin.world_tangent = vec4<f32>(frame.tangent, 1.0);
#endif
    }
    var pbr_input = pbr_input_from_standard_material(vin, is_front);

    let tag = unpack_tag(mesh_functions::get_tag(in.instance_index));
    let colour = srgb_to_linear(tag.rgb);
    // The material's colour (and colour map) are already in the input; the part tints them and scales their opacity.
    pbr_input.material.base_color = vec4<f32>(pbr_input.material.base_color.rgb * colour, pbr_input.material.base_color.a * tag.a * look.opacity);
    if look.emission_from_color > 0.5 {
        pbr_input.material.emissive = vec4<f32>(pbr_input.material.emissive.rgb * colour, pbr_input.material.emissive.a);
    }

#ifndef PREPASS_PIPELINE
    // Glass shows more of itself where it is seen at a slant.
    if look.edge_opacity > 0.0 {
        let facing = clamp(dot(pbr_input.N, pbr_input.V), 0.0, 1.0);
        let fresnel = pow(1.0 - facing, 5.0);
        let a = pbr_input.material.base_color.a;
        pbr_input.material.base_color.a = clamp(a + (1.0 - a) * fresnel * look.edge_opacity, 0.0, 1.0);
    }
#endif

    // Normal-map and occlusion strength (1 is as authored).
    if look.normal_strength != 1.0 {
        let flat_n = normalize(pbr_input.world_normal);
        pbr_input.N = normalize(flat_n + (pbr_input.N - flat_n) * look.normal_strength);
    }
    if look.occlusion_strength != 1.0 {
        pbr_input.diffuse_occlusion = mix(vec3<f32>(1.0), pbr_input.diffuse_occlusion, look.occlusion_strength);
        pbr_input.specular_occlusion = mix(1.0, pbr_input.specular_occlusion, look.occlusion_strength);
    }

    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

#ifdef PREPASS_PIPELINE
    let out = deferred_output(in, pbr_input);
#else
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
#endif

    return out;
}
