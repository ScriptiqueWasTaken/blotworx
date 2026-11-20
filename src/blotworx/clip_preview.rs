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
    for mesh in &world.player_meshes {
        let skin = world
            .skins
            .get(&mesh.asset)
            .and_then(|s| s.iter().find(|s| s.block == mesh.block))
            .context("Player mesh has no decoded skin")?;
        let palette = skin::palette_from_model_pose(skin, pose)?;
        let draw = draws.get_mut(mesh.draw).context("Missing player draw")?;
        let (vertices, minimum, maximum) =
            skin::render_skinned_pose(skin,&palette,&draw.v_data,&mesh.positions,&mesh.normals,&mesh.sources,mesh.transform)?;
        draw.v_data = Arc::new(vertices);
        draw.aabb_min = minimum;
        draw.aabb_max = maximum;
    }
    Ok(())
}
