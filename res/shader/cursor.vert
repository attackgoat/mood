#version 460 core

const vec4 POSITION_TEXTURE[6] = {
    vec4(-1, -1, 0, 0),
    vec4(-1,  1, 0, 1),
    vec4( 1,  1, 1, 1),
    vec4(-1, -1, 0, 0),
    vec4( 1,  1, 1, 1),
    vec4( 1, -1, 1, 0),
};

layout(push_constant) uniform PushConstants {
    vec2 position;
    vec2 scale;
} push_const;

layout(location = 0) out vec2 texture0_out;

void main() {
    vec4 position_texture = POSITION_TEXTURE[gl_VertexIndex];
    vec2 position = position_texture.xy;
    vec2 texture0 = position_texture.zw;

    gl_Position = vec4(position * push_const.scale + push_const.position, 0, 1);
    texture0_out = texture0;
}
