const uint8_t MESH_FLAGS_INDEX_TYPE_UINT32 = uint8_t(1);
const uint8_t MESH_FLAGS_JOINTS_WEIGHTS = uint8_t(2);

struct Mesh {
    uint32_t index_count;
    uint32_t index_offset;
    uint32_t vertex_offset;
    uint8_t material_idx;
    uint8_t flags;
    uint8_t vertex_stride;
    uint8_t _0;
};

struct Vertex {
    vec3 position;
    vec3 normal;
    vec4 tangent;
    vec2 texture0;
};
