#version 460 core
#extension GL_EXT_nonuniform_qualifier : require
#extension GL_EXT_shader_explicit_arithmetic_types_int8 : require
#extension GL_EXT_shader_explicit_arithmetic_types_int32 : require

#include "../material.glsl"

layout(binding = 0) uniform CameraBuffer {
    mat4 projection_view;
} camera;

layout(binding = 8) restrict readonly buffer MaterialBuffer {
    Material[] material_buf;
};

layout(binding = 9) uniform sampler2D texture_sampler_llr[];

layout(location = 0) in vec3 world_position;
layout(location = 1) in vec3 world_normal;
layout(location = 2) in vec2 texture0;
layout(location = 3) flat in uint material_idx;

layout(location = 0) out vec4 color_out;

void main() {
    Material material = material_buf[material_idx];

    color_out = texture(texture_sampler_llr[nonuniformEXT(material.color_idx)], texture0);

    float lit = dot(normalize(vec3(0.2, 1, 0)), world_normal);
    //color_out.rgb = vec3(1);
    color_out.rgb *= world_normal;

    //vec3 camera_dir = normalize(camera.position);
    //float light = abs(dot(ubo.camera_pos, normal));
}
