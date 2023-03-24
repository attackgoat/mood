#version 460 core

#include "quad.glsl"

layout(push_constant) uniform PushConstants {
    layout(offset = 0) mat4 vertex_transform;
} push_constants;

layout(location = 0) out vec2 texcoord_out;

void main() {
    texcoord_out = vertex_tex();
    gl_Position = push_constants.vertex_transform * vec4(vertex_pos(), 0, 1);
}
