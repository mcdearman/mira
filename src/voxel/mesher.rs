use glam::{IVec3, Vec3};

use super::{
    block::{BlockId, BlockRegistry},
    chunk::{ChunkData, CHUNK_SIZE},
    world::VoxelWorld,
};

/// 24 bytes per vertex. `packed` holds the face index (bits 0-2), the ambient occlusion level
/// (bits 3-4) and the texture layer (the rest); the shader looks the normal up from the face.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct VoxelVertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    pub packed: u32,
}

impl VoxelVertex {
    pub const ATTRIBUTES: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Uint32];

    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ChunkMesh {
    pub vertices: Vec<VoxelVertex>,
    pub indices: Vec<u32>,
}

/// Normal, then the two axes spanning the face. `u × v = normal` keeps the winding
/// counter-clockwise, and `v` points up on side faces so textures are upright.
/// The order matches `Block::layers` and the table in `voxel.wgsl`.
pub const FACES: [(IVec3, IVec3, IVec3); 6] = [
    (IVec3::X, IVec3::NEG_Z, IVec3::Y),
    (IVec3::NEG_X, IVec3::Z, IVec3::Y),
    (IVec3::Y, IVec3::Z, IVec3::X),
    (IVec3::NEG_Y, IVec3::X, IVec3::Z),
    (IVec3::Z, IVec3::X, IVec3::Y),
    (IVec3::NEG_Z, IVec3::NEG_X, IVec3::Y),
];

const CORNERS: [(i32, i32); 4] = [(-1, -1), (1, -1), (1, 1), (-1, 1)];

const PADDED: i32 = CHUNK_SIZE + 2;

/// A copy of a chunk plus a one-block border from its 26 neighbours. It owns its data, so it
/// can be meshed on another thread while the world keeps changing.
pub struct PaddedChunk {
    blocks: Vec<BlockId>,
}

impl PaddedChunk {
    pub fn capture(world: &VoxelWorld, chunk: IVec3) -> Self {
        let mut neighbours: [Option<&ChunkData>; 27] = [None; 27];
        let slot =
            |offset: IVec3| (((offset.z + 1) * 3 + offset.y + 1) * 3 + offset.x + 1) as usize;
        for dz in -1..=1 {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let offset = IVec3::new(dx, dy, dz);
                    neighbours[slot(offset)] = world.chunk(chunk + offset);
                }
            }
        }

        let size = IVec3::splat(CHUNK_SIZE);
        let mut blocks = Vec::with_capacity((PADDED * PADDED * PADDED) as usize);
        for y in -1..=CHUNK_SIZE {
            for z in -1..=CHUNK_SIZE {
                for x in -1..=CHUNK_SIZE {
                    let position = IVec3::new(x, y, z);
                    let source = neighbours[slot(position.div_euclid(size))];
                    blocks.push(
                        source.map_or(BlockId::UNLOADED, |c| c.get(position.rem_euclid(size))),
                    );
                }
            }
        }
        Self { blocks }
    }

    /// `local` may be anywhere from -1 to `CHUNK_SIZE` on each axis.
    fn get(&self, local: IVec3) -> BlockId {
        let p = local + 1;
        self.blocks[((p.y * PADDED + p.z) * PADDED + p.x) as usize]
    }
}

/// A block's face as it is to be drawn: its texture, and how lit each of its corners is.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Face {
    layer: u32,
    ao: [u32; 4],
}

impl Face {
    /// Whether the face is lit the same at every corner. Only such faces are joined: the
    /// light across one that is not runs from corner to corner of that one block, and would
    /// run across the whole of a larger quad instead.
    fn even(&self) -> bool {
        self.ao.iter().all(|corner| *corner == self.ao[0])
    }
}

/// Builds a mesh with hidden faces removed and per-vertex ambient occlusion.
/// Vertices are in world space, so chunks need no per-draw transform.
///
/// Faces that lie side by side in one plane, with the same texture and evenly lit, are
/// joined into one quad as wide and as high as they go (greedy meshing): flat ground and
/// walls come out as a few large quads, where each block had four vertices of its own. The
/// texture repeats across a joined quad, once a block.
pub fn mesh_chunk(padded: &PaddedChunk, registry: &BlockRegistry, chunk: IVec3) -> ChunkMesh {
    let mut mesh = ChunkMesh::default();
    let origin = chunk * CHUNK_SIZE;
    let solid = |local: IVec3| registry.is_solid(padded.get(local));
    let size = CHUNK_SIZE as usize;
    let mut faces: Vec<Option<Face>> = vec![None; size * size];

    for (face, (normal, u, v)) in FACES.iter().enumerate() {
        // The blocks of a slice are counted along the face's own two axes, from the corner
        // those axes lead away from.
        let corner = (u.min(IVec3::ZERO) + v.min(IVec3::ZERO)) * -(CHUNK_SIZE - 1);
        for depth in 0..CHUNK_SIZE {
            let block_at = |i: usize, j: usize| normal.abs() * depth + corner + *u * i as i32 + *v * j as i32;
            // What there is to draw in this slice.
            let mut any = false;
            for j in 0..size {
                for i in 0..size {
                    let local = block_at(i, j);
                    let id = padded.get(local);
                    let front = local + *normal;
                    faces[j * size + i] = match registry.get(id).filter(|_| id != BlockId::AIR && !solid(front)) {
                        None => None,
                        Some(block) => {
                            // A corner gets darker the more of its three neighbours in the
                            // layer in front of the face are solid. 3 is fully lit, 0 is
                            // fully occluded.
                            let ao = CORNERS.map(|(su, sv)| {
                                let (side_a, side_b) = (solid(front + *u * su), solid(front + *v * sv));
                                if side_a && side_b {
                                    0
                                } else {
                                    3 - (side_a as u32 + side_b as u32 + solid(front + *u * su + *v * sv) as u32)
                                }
                            });
                            any = true;
                            Some(Face { layer: block.layers[face], ao })
                        }
                    };
                }
            }
            if !any {
                continue;
            }
            for j in 0..size {
                for i in 0..size {
                    let Some(first) = faces[j * size + i] else { continue };
                    // As far along the row as the faces are the same, then as far up as whole
                    // rows of them are.
                    let (mut wide, mut high) = (1, 1);
                    if first.even() {
                        while i + wide < size && faces[j * size + i + wide] == Some(first) {
                            wide += 1;
                        }
                        while j + high < size && (i..i + wide).all(|across| faces[(j + high) * size + across] == Some(first)) {
                            high += 1;
                        }
                    }
                    for up in j..j + high {
                        faces[up * size + i..up * size + i + wide].fill(None);
                    }

                    let base = mesh.vertices.len() as u32;
                    for (at, (su, sv)) in CORNERS.iter().enumerate() {
                        // The block the corner belongs to, and the corner of that block.
                        let block = block_at(if *su < 0 { i } else { i + wide - 1 }, if *sv < 0 { j } else { j + high - 1 });
                        let center = (origin + block).as_vec3() + 0.5 + normal.as_vec3() * 0.5;
                        let offset = (*u * *su + *v * *sv).as_vec3() * 0.5;
                        mesh.vertices.push(VoxelVertex {
                            position: (center + offset).into(),
                            uv: [(*su + 1) as f32 * 0.5 * wide as f32, (1 - *sv) as f32 * 0.5 * high as f32],
                            packed: face as u32 | first.ao[at] << 3 | first.layer << 5,
                        });
                    }
                    // Split the quad along the diagonal that keeps the shading symmetric;
                    // otherwise a single dark corner smears across the whole face.
                    let ao = first.ao;
                    let order = if ao[0] + ao[2] < ao[1] + ao[3] { [1, 2, 3, 1, 3, 0] } else { [0, 1, 2, 0, 2, 3] };
                    mesh.indices.extend(order.map(|corner| base + corner));
                }
            }
        }
    }
    mesh
}
/// The world-space bounds of a chunk, for culling.
pub fn chunk_bounds(chunk: IVec3) -> (Vec3, Vec3) {
    let min = (chunk * CHUNK_SIZE).as_vec3();
    (min, min + CHUNK_SIZE as f32)
}
