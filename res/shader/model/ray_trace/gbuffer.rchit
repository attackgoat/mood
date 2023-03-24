#version 460
#extension GL_EXT_nonuniform_qualifier : require
#extension GL_EXT_ray_tracing : require
#extension GL_EXT_shader_explicit_arithmetic_types_float32 : require
#extension GL_EXT_shader_explicit_arithmetic_types_int8 : require
#extension GL_EXT_shader_explicit_arithmetic_types_int16 : require
#extension GL_EXT_shader_explicit_arithmetic_types_int32 : require

#include "../material.glsl"
#include "../mesh.glsl"
#include "model_instance.glsl"
#include "ray_payload.glsl"

layout(binding = 2) buffer Index16Buffer {
    uint16_t[] index16_buf;
};

layout(binding = 2) buffer Index32Buffer {
    uint32_t[] index32_buf;
};

layout(binding = 2) buffer VertexBuffer {
    float32_t[] vertex_buf;
};

layout(binding = 3) buffer MaterialBuffer {
    Material[] material_buf;
};

layout(binding = 4) buffer MeshBuffer {
    Mesh[] mesh_buf;
};

layout(binding = 6) buffer ModelInstanceBuffer {
    ModelInstance[] model_instance_buf;
};

layout(binding = 7) uniform sampler2D texture_sampler_llr[];

hitAttributeEXT vec2 hit_bary_coord;

layout(location = 0) rayPayloadInEXT RayPayload ray_payload_in;

#include "../mesh_fns.glsl"

vec3 barycentric_weight(vec2 bary_coord) {
    return vec3(1.0 - bary_coord.x - bary_coord.y,
                bary_coord.x,
                bary_coord.y);
}

void main() {
    const ModelInstance model_instance = model_instance_buf[gl_InstanceCustomIndexEXT];
    const Mesh mesh = mesh_buf[model_instance.mesh_index + gl_GeometryIndexEXT];
    const uint material_index = uint(model_instance.material_indices[mesh.material_idx]);
    const Material material = material_buf[material_index];

    const uvec3 indices = mesh_triangle_indices(mesh, gl_PrimitiveID);
    const Vertex v0 = mesh_vertex(mesh, indices.x);
    const Vertex v1 = mesh_vertex(mesh, indices.y);
    const Vertex v2 = mesh_vertex(mesh, indices.z);

    const vec3 hit_bary_weight = barycentric_weight(hit_bary_coord);
    vec3 hit_position = v0.position * hit_bary_weight.x
                      + v1.position * hit_bary_weight.y
                      + v2.position * hit_bary_weight.z;
    vec2 hit_texture0 = v0.texture0 * hit_bary_weight.x
                      + v1.texture0 * hit_bary_weight.y
                      + v2.texture0 * hit_bary_weight.z;
    vec3 hit_normal = normalize(cross(v1.position - v0.position, v2.position - v0.position));

    vec4 hit_color = texture(texture_sampler_llr[material.color_idx], hit_texture0);

    ray_payload_in.color = hit_color.xyz * hit_normal;
}
