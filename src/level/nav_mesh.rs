use {
    glam::{vec3, Mat4, Quat, Vec2, Vec3},
    std::collections::HashMap,
};

fn closest_point_triangle(p: Vec3, [a, b, c]: [Vec3; 3]) -> ClosestPoint {
    // From implementation described in Real-Time Collision Detection by Christer Ericson 2005

    let ab = b - a;
    let ac = c - a;
    let ap = p - a;
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);

    // Check if P in vertex region outside A
    if d1 <= 0.0 && d2 <= 0.0 {
        // barycentric coordinates (1, 0, 0)
        return ClosestPoint::Vertex(0);
    }

    let bp = p - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);

    // Check if P in vertex region outside B
    if d3 >= 0.0 && d4 <= d3 {
        // barycentric coordinates (0, 1, 0)
        return ClosestPoint::Vertex(1);
    }

    let vc = d1 * d4 - d3 * d2;

    // Check if P in edge region of AB, if so return projection of P onto AB
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);

        // barycentric coordinates (1 - v, v, 0)
        return ClosestPoint::Edge(0, a + v * ab);
    }

    let cp = p - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);

    // Check if P in vertex region outside C
    if d6 >= 0.0 && d5 <= d6 {
        // barycentric coordinates (0, 0, 1)
        return ClosestPoint::Vertex(2);
    }

    // Check if P in edge region of AC, if so return projection of P onto AC
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);

        // barycentric coordinates (1 - w, 0, w)
        return ClosestPoint::Edge(2, a + w * ac);
    }

    let va = d3 * d6 - d5 * d4;

    // Check if P in edge region of BC, if so return projection of P onto BC
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));

        // barycentric coordinates (0, 1 - w, w)
        return ClosestPoint::Edge(1, b + w * (c - b));
    }

    let denom = 1.0 / (va + vb + vc);
    let v = vb * denom;
    let w = vc * denom;

    // P inside face region. Compute Q through its barycentric coordinates (u, v, w)
    ClosestPoint::Face(a + ab * v + ac * w)
}

fn triangle_neighbors(triangle_indices: &[[usize; 3]]) -> Vec<NeighborIndices> {
    #[derive(Clone, Copy, Eq, Hash, PartialEq)]
    struct Edge([usize; 2]);

    let mut edges = HashMap::with_capacity(triangle_indices.len() * 3);
    let mut corners = HashMap::with_capacity(triangle_indices.len() * 6);

    let mut insert_edge = |i, j, index| {
        edges.insert(Edge([i, j]), index);
    };
    let mut insert_corner = |i, index| {
        corners
            .entry(i)
            .and_modify(|indices: &mut Vec<usize>| indices.push(index))
            .or_insert_with(|| vec![index]);
    };

    for (triangle_index, [a, b, c]) in triangle_indices.iter().copied().enumerate() {
        insert_edge(a, b, triangle_index);
        insert_edge(b, c, triangle_index);
        insert_edge(c, a, triangle_index);
        insert_corner(a, triangle_index);
        insert_corner(b, triangle_index);
        insert_corner(c, triangle_index);
    }

    let mut res = Vec::with_capacity(triangle_indices.len());
    for (triangle_index, [a, b, c]) in triangle_indices.iter().copied().enumerate() {
        let ba = edges.get(&Edge([b, a])).copied();
        let cb = edges.get(&Edge([c, b])).copied();
        let ac = edges.get(&Edge([a, c])).copied();

        res.push(NeighborIndices {
            corners: [
                corners[&a]
                    .iter()
                    .copied()
                    .filter(|i| {
                        *i != triangle_index
                            && ba.filter(|j| j == i).is_none()
                            && ac.filter(|j| j == i).is_none()
                    })
                    .collect(),
                corners[&b]
                    .iter()
                    .copied()
                    .filter(|i| {
                        *i != triangle_index
                            && ba.filter(|j| j == i).is_none()
                            && cb.filter(|j| j == i).is_none()
                    })
                    .collect(),
                corners[&c]
                    .iter()
                    .copied()
                    .filter(|i| {
                        *i != triangle_index
                            && cb.filter(|j| j == i).is_none()
                            && ac.filter(|j| j == i).is_none()
                    })
                    .collect(),
            ],
            edges: [ba, cb, ac],
        });
    }

    debug_assert_eq!(triangle_indices.len(), res.len());

    res
}

enum ClosestPoint {
    Edge(usize, Vec3),
    Face(Vec3),
    Vertex(usize),
}

#[derive(Clone, Copy, Debug)]
pub struct MeshLocation {
    triangle_index: usize,
    position: Vec3,
}

impl MeshLocation {
    /// Returns the world position of this location.
    pub fn position(&self) -> Vec3 {
        self.position
    }
}

/// Defines a navigable x/z plane built off the data of a mesh.
pub struct NavigationMesh {
    neighbor_indices: Vec<NeighborIndices>,
    triangle_indices: Vec<[usize; 3]>,
    vertices: Vec<Vec3>,
}

impl NavigationMesh {
    /// Constructs a new navigation mesh given a set of position vertices and their indices which
    /// define a triangulated mesh. Faces are clockwise, given as triangle indices a-b-c.
    pub fn new(indices: &[u32], vertices: &[Vec3]) -> Self {
        debug_assert_eq!(indices.len() % 3, 0);
        debug_assert!(!indices.is_empty());
        debug_assert!(indices
            .iter()
            .copied()
            .all(|index| (index as usize) < vertices.len()));

        let triangle_count = indices.len() / 3;
        let mut triangle_indices = Vec::with_capacity(triangle_count);
        for triangle_index in 0..triangle_count {
            let index_offset = triangle_index * 3;
            let indices = &indices[index_offset..];
            triangle_indices.push([indices[0] as _, indices[1] as _, indices[2] as _]);
        }

        Self {
            neighbor_indices: triangle_neighbors(&triangle_indices),
            triangle_indices,
            vertices: vertices.iter().copied().collect(),
        }
    }

    /// Gets the navigable position closest to the given world position.
    ///
    /// Returns a location which has been clamped to the mesh surface.
    pub fn locate(&self, mut position: Vec3) -> MeshLocation {
        let mut triangle_index = 0;
        let mut best_distance_squared = f32::MAX;
        let mut best_position = Vec3::ZERO;

        for (current_triangle_index, [a, b, c]) in self.triangle_indices.iter().copied().enumerate()
        {
            let triangle = [self.vertices[a], self.vertices[b], self.vertices[c]];
            let closest_point = match closest_point_triangle(position, triangle) {
                ClosestPoint::Edge(_, p) | ClosestPoint::Face(p) => p,
                ClosestPoint::Vertex(i) => triangle[i],
            };
            let distance_squared = position.distance_squared(closest_point);

            if distance_squared < best_distance_squared {
                best_distance_squared = distance_squared;
                triangle_index = current_triangle_index;
                best_position = closest_point;
            }
        }

        MeshLocation {
            triangle_index,
            position: best_position,
        }
    }

    /// Returns the normal of the mesh surface at the given location.
    pub fn surface_normal(&self, location: MeshLocation) -> Vec3 {
        let [a, b, c] = self.triangle_indices[location.triangle_index];
        let [v0, v1, v2] = [self.vertices[a], self.vertices[b], self.vertices[c]];

        let i = v1 - v0;
        let j = v2 - v0;

        i.cross(j).normalize()
    }

    /// Walks in relation to the current location, returning the new location
    ///
    /// The direction parameter is in world coordinates.
    pub fn walk(&mut self, mut location: MeshLocation, direction: Vec2) -> MeshLocation {
        let target = location.position + vec3(direction.x, 0.0, direction.y);
        let mut distance_remaining = direction.distance_squared(Vec2::ZERO);

        while distance_remaining > 0.0 {
            let current_triangle = {
                let [a, b, c] = self.triangle_indices[location.triangle_index];
                [self.vertices[a], self.vertices[b], self.vertices[c]]
            };

            match closest_point_triangle(target, current_triangle) {
                ClosestPoint::Edge(edge, position) => {
                    if let Some(triangle_index) =
                        self.neighbor_indices[location.triangle_index].edges[edge]
                    {
                        location.triangle_index = triangle_index;
                    }

                    location.position = position;
                }
                ClosestPoint::Face(position) => {
                    location.position = position;
                    break;
                }
                ClosestPoint::Vertex(vertex) => {
                    let mut best_distance = 0.0;
                    let start_position = location.position;
                    for triangle_index in self.neighbor_indices[location.triangle_index].corners
                        [vertex]
                        .iter()
                        .copied()
                    {
                        let triangle = {
                            let [a, b, c] = self.triangle_indices[triangle_index];
                            [self.vertices[a], self.vertices[b], self.vertices[c]]
                        };
                        let position = match closest_point_triangle(target, triangle) {
                            ClosestPoint::Edge(_, p) | ClosestPoint::Face(p) => p,
                            ClosestPoint::Vertex(i) => triangle[i],
                        };

                        let distance = (start_position - position).dot(start_position - target);
                        if distance > best_distance {
                            best_distance = distance;
                            location.position = position;
                            location.triangle_index = triangle_index;
                        }
                    }
                }
            }

            distance_remaining -= target.distance_squared(location.position);
        }

        location
    }
}

struct NeighborIndices {
    corners: [Vec<usize>; 3],
    edges: [Option<usize>; 3],
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_approx(lhs: f32, rhs: f32) {
        assert!(
            lhs.is_finite() && rhs.is_finite() && (lhs - rhs).abs() < f32::EPSILON,
            "{lhs} is not approximately {rhs}"
        );
    }

    #[test]
    pub fn closest_point() {
        let vertices = [
            vec3(-9.0, 0.049999997, 6.0),
            vec3(-7.0, 0.049999997, 6.0),
            vec3(-7.0, 0.049999997, 2.0),
        ];
        let p = vec3(-8.0, 1.8, 5.0);
        let cp = closest_point_triangle(p, vertices);

        if let ClosestPoint::Face(cp) = cp {
            assert_approx(cp.x, -8.0);
            assert_approx(cp.y, 0.05);
            assert_approx(cp.z, 5.0);
        } else {
            assert!(false);
        }
    }

    #[test]
    pub fn locate() {
        let vertices = [
            vec3(-9.0, 0.049999997, 6.0),
            vec3(-7.0, 0.049999997, 6.0),
            vec3(-9.0, 0.049999997, 2.0),
            vec3(-7.0, 0.049999997, 2.0),
        ];
        let indices = [0, 1, 3, 0, 3, 2];

        let nav_mesh = NavigationMesh::new(&indices, &vertices);
        let location = nav_mesh.locate(vec3(-8.0, 1.8, 5.0));

        assert_approx(location.position().x, -8.0);
        assert_approx(location.position().y, 0.05);
        assert_approx(location.position().z, 5.0);
    }

    #[test]
    pub fn triangle_neighbor_indices() {
        //
        // v0--v1
        // |t0/ | \
        // | /t1|t2\
        // v2--v3--v4
        //     | \t3|
        //     |t4\ |
        //     v5--v6

        let triangle_indices = [[0, 1, 2], [1, 3, 2], [1, 4, 3], [3, 4, 6], [3, 6, 5]];

        let res = triangle_neighbors(&triangle_indices);

        assert_eq!(res.len(), 5);

        let empty = Vec::<usize>::new();

        assert_eq!(res[0].corners[0], empty);
        assert_eq!(res[0].corners[1], vec![2]);
        assert_eq!(res[0].corners[2], empty);
        assert_eq!(res[0].edges[0], None);
        assert_eq!(res[0].edges[1], Some(1));
        assert_eq!(res[0].edges[2], None);

        assert_eq!(res[1].corners[0], empty);
        assert_eq!(res[1].corners[1], vec![3, 4]);
        assert_eq!(res[1].corners[2], empty);
        assert_eq!(res[1].edges[0], Some(2));
        assert_eq!(res[1].edges[1], None);
        assert_eq!(res[1].edges[2], Some(0));

        assert_eq!(res[2].corners[0], vec![0]);
        assert_eq!(res[2].corners[1], empty);
        assert_eq!(res[2].corners[2], vec![4]);
        assert_eq!(res[2].edges[0], None);
        assert_eq!(res[2].edges[1], Some(3));
        assert_eq!(res[2].edges[2], Some(1));

        assert_eq!(res[3].corners[0], vec![1]);
        assert_eq!(res[3].corners[1], empty);
        assert_eq!(res[3].corners[2], empty);
        assert_eq!(res[3].edges[0], Some(2));
        assert_eq!(res[3].edges[1], None);
        assert_eq!(res[3].edges[2], Some(4));

        assert_eq!(res[4].corners[0], vec![1, 2]);
        assert_eq!(res[4].corners[1], empty);
        assert_eq!(res[4].corners[2], empty);
        assert_eq!(res[4].edges[0], Some(3));
        assert_eq!(res[4].edges[1], None);
        assert_eq!(res[4].edges[2], None);
    }
}
