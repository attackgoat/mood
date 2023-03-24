#version 460
#extension GL_EXT_ray_tracing : require

#include "ray_payload.glsl"

layout(location = 0) rayPayloadInEXT RayPayload ray_payload_in;

void main() {
    ray_payload_in.color = vec3(0.0, 0.0, 1.0);
}
