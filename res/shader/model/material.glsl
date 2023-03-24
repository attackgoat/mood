const uint8_t MATERIAL_FLAGS_EMISSIVE = uint8_t(1);

struct Material {
    uint32_t color_idx;
    uint8_t flags;
    uint8_t[3] _0;
};
