//! A tiny accumulator for hand-built, **flat-shaded** low-poly geometry. Every triangle gets
//! its own face normal (the faceted look the whole style leans on) and a vertex colour, so a
//! single white material can carry an entire scene. Shared by the ground builder and every
//! procedural prop generator.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

#[derive(Default)]
pub struct MeshBuf {
    pos: Vec<[f32; 3]>,
    nor: Vec<[f32; 3]>,
    col: Vec<[f32; 4]>,
    idx: Vec<u32>,
}

impl MeshBuf {
    fn vert(&mut self, p: Vec3, n: Vec3, c: [f32; 4]) -> u32 {
        let i = self.pos.len() as u32;
        self.pos.push([p.x, p.y, p.z]);
        self.nor.push([n.x, n.y, n.z]);
        self.col.push(c);
        i
    }

    /// A flat-shaded triangle (its own face normal), wound counter-clockwise when seen from
    /// the front.
    pub fn tri(&mut self, a: Vec3, b: Vec3, c: Vec3, color: [f32; 3]) {
        let n = (b - a).cross(c - a).normalize_or_zero();
        let rgba = [color[0], color[1], color[2], 1.0];
        let i = self.vert(a, n, rgba);
        let j = self.vert(b, n, rgba);
        let k = self.vert(c, n, rgba);
        self.idx.extend([i, j, k]);
    }

    /// A flat-shaded quad `a→b→c→d` (two triangles).
    pub fn quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3, color: [f32; 3]) {
        self.tri(a, b, c, color);
        self.tri(a, c, d, color);
    }

    /// Every triangle as its three positions and (flat) face normal — for tests that check
    /// winding/orientation.
    #[cfg(test)]
    pub fn tris(&self) -> Vec<([Vec3; 3], Vec3)> {
        self.idx
            .chunks(3)
            .map(|t| {
                let p = |i: u32| Vec3::from_array(self.pos[i as usize]);
                ([p(t[0]), p(t[1]), p(t[2])], Vec3::from_array(self.nor[t[0] as usize]))
            })
            .collect()
    }

    pub fn into_mesh(self) -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.nor)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.col)
            .with_inserted_indices(Indices::U32(self.idx))
    }
}
