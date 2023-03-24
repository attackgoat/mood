#version 450
#extension GL_EXT_shader_explicit_arithmetic_types_float32 : require
#extension GL_EXT_shader_explicit_arithmetic_types_int8 : require
#extension GL_EXT_shader_explicit_arithmetic_types_int16 : require
#extension GL_EXT_shader_explicit_arithmetic_types_int32 : require

#include "../../quat.glsl"
#include "../mesh.glsl"
#include "mesh_instance.glsl"
#include "model_instance.glsl"

layout(binding = 0) uniform CameraUniform {
    mat4 projection_view;
} camera;

layout(binding = 1) restrict readonly buffer DrawInstanceBuffer {
    uint32_t[] draw_instance_buf;
};

layout(binding = 2) buffer Index16Buffer {
    uint16_t[] index16_buf;
};

layout(binding = 3) buffer Index32Buffer {
    uint32_t[] index32_buf;
};

layout(binding = 4) buffer VertexBuffer {
    float32_t[] vertex_buf;
};

layout(binding = 5) restrict readonly buffer MeshInstanceBuffer {
    MeshInstance[] mesh_instance_buf;
};

layout(binding = 6) restrict readonly buffer MeshBuffer {
    Mesh[] mesh_buf;
};

layout(binding = 7) restrict readonly buffer ModelInstanceBuffer {
    ModelInstance[] model_instance_buf;
};

#include "../mesh_fns.glsl"

layout(location = 0) out vec3 world_position_out;
layout(location = 1) out vec3 world_normal_out;
layout(location = 2) out vec2 texture_out;
layout(location = 3) flat out uint material_idx_out;

void main() {
    uint mesh_instance_idx = draw_instance_buf[gl_InstanceIndex];
    MeshInstance mesh_instance = mesh_instance_buf[mesh_instance_idx];
    Mesh mesh = mesh_buf[mesh_instance.mesh_idx];
    ModelInstance model_instance = model_instance_buf[mesh_instance.model_instance_idx];
    uint material_idx = model_instance.material_indices[mesh.material_idx];

    uint vertex_index = mesh_vertex_index(mesh, gl_VertexIndex);
    Vertex vertex = mesh_vertex(mesh, vertex_index);

    world_normal_out = quat_transform(model_instance.rotation, vertex.normal);
    world_position_out = quat_transform(model_instance.rotation, vertex.position)
                       + model_instance.translation;

    texture_out = vertex.texture0;

    material_idx_out = material_idx;

    gl_Position = camera.projection_view
                * vec4(world_position_out, 1.0);
}
