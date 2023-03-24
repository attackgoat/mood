#version 460 core

layout(binding = 0) uniform sampler2D cursor_sampler_nne;

layout(location = 0) in vec2 texture0;

layout(location = 0) out vec4 color;

void main() {
    color = texture(cursor_sampler_nne, texture0);
}
