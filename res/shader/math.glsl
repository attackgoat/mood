float distance_sq(vec3 a, vec3 b) {
    vec3 c = a - b;

    return dot(c, c);
}
