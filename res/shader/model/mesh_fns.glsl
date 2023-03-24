uvec3 mesh_triangle_indices(Mesh mesh, uint primitive_index) {
    const uint base_index = 3 * primitive_index + mesh.index_offset;

    if ((mesh.flags & MESH_FLAGS_INDEX_TYPE_UINT32) != 0) {
        return uvec3(index32_buf[base_index],
                     index32_buf[base_index + 1],
                     index32_buf[base_index + 2]);
    } else {
        return uvec3(index16_buf[base_index],
                     index16_buf[base_index + 1],
                     index16_buf[base_index + 2]);
    }
}

uint mesh_vertex_index(Mesh mesh, uint index) {
    const uint base_index = index + mesh.index_offset;

    if ((mesh.flags & MESH_FLAGS_INDEX_TYPE_UINT32) != 0) {
        return uint(index32_buf[base_index]);
    } else {
        return uint(index16_buf[base_index]);
    }
}

Vertex mesh_vertex(Mesh mesh, uint index) {
    uint offset = index * mesh.vertex_stride + mesh.vertex_offset;

    Vertex vertex;

    vertex.position = vec3(vertex_buf[offset],
                           vertex_buf[offset + 1],
                           vertex_buf[offset + 2]);

    vertex.normal = vec3(vertex_buf[offset + 3],
                         vertex_buf[offset + 4],
                         vertex_buf[offset + 5]);

    vertex.tangent = vec4(vertex_buf[offset + 8],
                          vertex_buf[offset + 9],
                          vertex_buf[offset + 10],
                          vertex_buf[offset + 11]);

    vertex.texture0 = vec2(vertex_buf[offset + 6],
                           vertex_buf[offset + 7]);

    return vertex;
}
