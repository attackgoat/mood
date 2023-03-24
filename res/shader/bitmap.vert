#version 460 core
#extension GL_EXT_nonuniform_qualifier : require

struct Rect {
    uvec2 position;
    uvec2 size;
};

layout(push_constant) uniform PushConstants {
   Rect src;
   ivec2[2] dst;
   uvec2 color_size;
   uint atlas_idx;
} push_const;

layout(location = 0) out vec2 texture0_out;

void main() {
    vec2 atlas_size = vec2(2048);
    vec2 color_size = vec2(push_const.color_size);

    gl_Position = vec4(0, 0, 0, 1);

    switch (gl_VertexIndex) {
    case 0:
        gl_Position.xy = vec2(push_const.dst[0]);
        texture0_out = vec2(push_const.src.position);
        break;
    case 1:
        gl_Position.xy = vec2(push_const.dst[0].x, push_const.dst[0].y + push_const.dst[1].y);
        texture0_out = vec2(push_const.src.position.x, push_const.src.position.y + push_const.src.size.y);
        break;
    case 2:
        gl_Position.xy = vec2(push_const.dst[0] + push_const.dst[1]);
        texture0_out = vec2(push_const.src.position + push_const.src.size);
        break;
    case 3:
        gl_Position.xy = vec2(push_const.dst[0]);
        texture0_out = vec2(push_const.src.position);
        break;
    case 4:
        gl_Position.xy = vec2(push_const.dst[0] + push_const.dst[1]);
        texture0_out = vec2(push_const.src.position + push_const.src.size);
        break;
    case 5:
        gl_Position.xy = vec2(push_const.dst[0].x + push_const.dst[1].x, push_const.dst[0].y);
        texture0_out = vec2(push_const.src.position.x + push_const.src.size.x, push_const.src.position.y);
        break;
    }

    gl_Position.xy /= color_size;
    gl_Position.xy *= 2;
    gl_Position.xy -= 1;

    texture0_out /= atlas_size;
}
