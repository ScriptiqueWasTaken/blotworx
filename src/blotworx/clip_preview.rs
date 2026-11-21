//! Explicit original-clip inspection, not gameplay behavior selection.
use super::{assets::World, skin};
use crate::render_support::viewport3d::GpuDrawCommand;
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::hkx_file::{HkxFile, HkxSkeleton};
use std::{path::Path, sync::Arc};

pub struct ClipPreview {
    rig: HkxSkeleton,
    clip: super::animation_resources::Clip,
    pub time: f32,
}
impl ClipPreview {
    pub fn load(rig: &Path, clip: &Path) -> Result<Self> {
        let rig = HkxFile::load_bytes(&std::fs::read(rig)?)
            .map_err(anyhow::Error::msg)?
            .parse_skeleton()
            .context("Preview rig has no skeleton")?;
        let clip = super::animation_resources::Clip::load(&std::fs::read(clip)?, rig.bones.len())?;
        Ok(Self {
            rig,
            clip,
            time: 0.,
        })
    }
    pub fn advance(&mut self, elapsed: f32) {
        self.time = (self.time + elapsed).rem_euclid(self.clip.info.duration);
    }
    pub fn apply(&self, world: &World, draws: &mut [GpuDrawCommand]) -> Result<()> {
        ensure!(
            !world.player_meshes.is_empty(),
            "No player geometry available for clip preview"
        );
        let pose = self.clip.sample_bones(&self.rig, self.time)?;
        apply_pose(world, &pose, draws)
    }
}

pub fn apply_pose(
    world: &World,
    pose: &[epic_mickey_lib_rs::hkx_file::HkxBone],
    draws: &mut [GpuDrawCommand],
) -> Result<()> {
    apply_model_pose(world, &skin::model_pose(pose, &[])?, draws)
}

pub fn apply_model_pose(
    world: &World,
    pose: &[[f32; 16]],
    draws: &mut [GpuDrawCommand],
) -> Result<()> {
    apply_model_pose_inner(world, pose, draws, None)
}

#[derive(Default)]
pub(super) struct SkinFrame {
    pose: Vec<[f32; 16]>,
    meshes: std::collections::HashMap<usize, (Arc<Vec<f32>>, SkinnedMesh)>,
}

#[derive(Clone)]
pub(super) struct SkinnedMesh {
    vertices: Arc<Vec<f32>>,
    minimum: [f32; 3],
    maximum: [f32; 3],
}

impl SkinnedMesh {
    pub(super) fn new((vertices, minimum, maximum): (Vec<f32>, [f32; 3], [f32; 3])) -> Self {
        Self { vertices: Arc::new(vertices), minimum, maximum }
    }

    pub(super) fn apply(&self, draw: &mut GpuDrawCommand) {
        draw.v_data = self.vertices.clone();
        draw.aabb_min = self.minimum;
        draw.aabb_max = self.maximum;
    }
}

impl SkinFrame {
    pub(super) fn apply(&mut self, world: &World, pose: &[[f32; 16]], draws: &mut [GpuDrawCommand]) -> Result<()> {
        if self.pose != pose {
            self.pose.clear();
            self.pose.extend_from_slice(pose);
            self.meshes.clear();
        }
        apply_model_pose_inner(world, pose, draws, Some(self))
    }
}

fn apply_model_pose_inner(
    world: &World,
    pose: &[[f32; 16]],
    draws: &mut [GpuDrawCommand],
    mut cache: Option<&mut SkinFrame>,
) -> Result<()> {
    for mesh in &world.player_meshes {
        let draw = draws.get_mut(mesh.draw).context("Missing player draw")?;
        if let Some((_, skinned)) = cache.as_ref().and_then(|cache| cache.meshes.get(&mesh.draw))
            .filter(|(source, _)| Arc::ptr_eq(source, &draw.v_data))
        {
            skinned.apply(draw);
            continue;
        }
        let skin = world
            .skins
            .get(&mesh.asset)
            .and_then(|s| s.iter().find(|s| s.block == mesh.block))
            .context("Player mesh has no decoded skin")?;
        let palette = skin::palette_from_model_pose(skin, pose)?;
        let skinned = SkinnedMesh::new(
            skin::render_skinned_pose(skin,&palette,&draw.v_data,&mesh.positions,&mesh.normals,&mesh.sources,mesh.transform)?
        );
        if let Some(cache) = cache.as_mut() {
            cache.meshes.insert(mesh.draw, (draw.v_data.clone(), skinned.clone()));
        }
        skinned.apply(draw);
    }
    Ok(())
}
