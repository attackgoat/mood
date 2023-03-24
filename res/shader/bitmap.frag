#version 460 core
#extension GL_EXT_nonuniform_qualifier : require

layout(push_constant) uniform PushConstants {
    layout(offset = 40) uint atlas_idx;
} push_const;

layout(binding = 0) uniform sampler2D atlas_sampler_nne[];

layout(location = 0) in vec2 texture0;

layout(location = 0) out vec4 color_out;

void main() {
    color_out = texture(atlas_sampler_nne[push_const.atlas_idx], texture0);
}
