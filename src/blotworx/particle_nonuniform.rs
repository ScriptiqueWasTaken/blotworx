//! EM1's nonuniform particle subclass payloads and independent size channel.
//! REL14ebec/14e870,1516d8,1531d4 and152934. Original assets stay immutable.
use super::Reader;
use anyhow::{ensure, Result};

///14f460..14f4f8 initializes full-range UVs when no texture-seed stream
/// exists. Atlas dimensions don't divide these persistent vertex coordinates.
pub(super) fn unseeded_uvs() -> [[f32; 2]; 4] {
    [[0., 0.], [0., 1.], [1., 1.], [1., 0.]]
}

fn signed_sample(raw: u32) -> Result<f32> {
    ensure!(raw <= 32767, "Invalid nonuniform particle RNG result");
    Ok((2. * raw as f32) / 32767. - 1.)
}
fn atlas_state(raw: u32) -> Result<f32> {
    Ok(0.99 * signed_sample(raw)?)
}
pub(super) fn atlas_frame(raw: u32, atlas: [u16; 2]) -> Result<u32> {
    ensure!(
        atlas.into_iter().all(|n| n > 0),
        "Empty nonuniform particle atlas"
    );
    let count = u32::from(atlas[0]) * u32::from(atlas[1]);
    Ok((count as f32 * atlas_state(raw)?.abs()) as u32)
}

#[derive(Clone)]
pub(super) struct System {
    width: [f32; 2],
    height: [f32; 2],
    ratio: f32,
    linked: bool,
    velocity: Option<[f32; 6]>,
}
impl System {
    pub fn load(bytes: &[u8], little: bool, user: u32) -> Result<Self> {
        let mut r = Reader {
            bytes,
            at: 0,
            little,
        };
        let revision = if user >= 5 { r.short()? } else { 0 };
        let width = [r.float()?, r.float()?];
        let height = [r.float()?, r.float()?];
        let ratio = r.float()?;
        let linked = r.byte()? != 0;
        let velocity = if revision == 1 {
            let velocity = r.byte()? != 0;
            let mut limits = [0.; 6];
            for value in &mut limits {
                *value = r.float()?;
            }
            // REL14fee0 publishes maxW/maxH/minW/minH/scaleW/scaleH.
            // The native update constrains each minimum to its maximum.
            limits[2] = limits[2].min(limits[0]);
            limits[3] = limits[3].min(limits[1]);
            velocity.then_some(limits)
        } else {
            None
        };
        r.done()?;
        Ok(Self {
            width,
            height,
            ratio,
            linked,
            velocity,
        })
    }
    /// REL151c60..151cf4 clamps animated factors, before birth radii multiply.
    pub fn animated_factors(&self, factors: [f32; 2], velocity: [f32; 3]) -> [f32; 2] {
        let Some(v) = self.velocity else {
            return factors;
        };
        let speed = velocity.iter().map(|x| x * x).sum::<f32>().sqrt();
        [
            (factors[0] * (v[4] * speed)).max(v[2]).min(v[0]),
            (factors[1] * (v[5] * speed)).max(v[3]).min(v[1]),
        ]
    }
    /// Raw15-bit samples are supplied by the caller. Both variances still
    /// consume a sample when zero; linked height consumes no second sample.
    pub fn sizes(&self, mut random: impl FnMut() -> u32) -> Result<[f32; 2]> {
        let mut sample = || signed_sample(random());
        let width = self.width[1].mul_add(sample()?, self.width[0]);
        let height = if self.linked {
            self.ratio * width
        } else {
            self.height[1].mul_add(sample()?, self.height[0])
        };
        ensure!(
            [width, height].into_iter().all(f32::is_finite),
            "Nonuniform particle birth overflow"
        );
        Ok([width, height])
    }
}

#[derive(Clone)]
pub(super) struct Track {
    keys: Vec<(f32, f32)>,
    mode: u32,
}
impl Track {
    pub fn read(r: &mut Reader, mode: u32) -> Result<Self> {
        let mut keys = Vec::new();
        for _ in 0..r.byte()? {
            keys.push((r.float()?, r.float()?));
        }
        ensure!(
            mode <= 2,
            "Nonuniform size channel time mode {mode} requires its native wrap/reverse helper"
        );
        ensure!(
            keys.windows(2).all(|pair| pair[0].1 < pair[1].1),
            "Unordered nonuniform size keys"
        );
        Ok(Self { keys, mode })
    }
    pub fn sample(&self, age: f32, life: f32) -> Result<f32> {
        ensure!(
            age.is_finite() && life.is_finite() && life > 0.,
            "Invalid nonuniform size clock"
        );
        let value = match self.keys.as_slice() {
            [] => 1.,
            [(value, _)] => *value,
            keys => {
                let first = keys[0];
                let last = keys[keys.len() - 1];
                let time = match self.mode {
                    0 => age + first.1,
                    1 => age + (last.1 - life),
                    2 => first.1 + (age * (last.1 - first.1)) / life,
                    _ => anyhow::bail!("Unsupported nonuniform time mode"),
                };
                if time <= first.1 {
                    first.0
                } else if time >= last.1 {
                    last.0
                } else {
                    let next = keys.partition_point(|key| key.1 < time);
                    let a = keys[next - 1];
                    let b = keys[next];
                    let factor = (time - a.1) / (b.1 - a.1);
                    (1. - factor).mul_add(a.0, factor * b.0)
                }
            }
        };
        ensure!(value.is_finite(), "Nonuniform size channel overflow");
        Ok(if value < 1e-4 { 1e-4 } else { value })
    }
}

pub(super) struct Generator {
    pub center: [f32; 2],
    pub atlas: [u16; 2],
    pub frame_variation: f32,
    pub animated: bool,
    pub start_frame: u16,
    pub frame_count: u16,
    pub frame_rate: f32,
}
impl Generator {
    /// Reader starts after the shared NiMeshModifier submit/complete prefix.
    pub fn read(r: &mut Reader, user: u32, texture_seeds: bool) -> Result<Self> {
        let (revision, alternate, mut center) = if user >= 8 {
            (r.short()?, r.byte()? != 0, [r.float()?, r.float()?])
        } else {
            (0, false, [0.; 2])
        };
        let legacy = [r.float()?, r.float()?];
        if revision == 0 {
            center = legacy;
        }
        ensure!(
            !alternate,
            "Nonuniform alternate quad UV mode is not implemented"
        );
        let scrolling = r.byte()?;
        let atlas = [r.short()?, r.short()?];
        ensure!(
            atlas.into_iter().all(|n| n > 0),
            "Empty nonuniform particle atlas"
        );
        let animated = r.byte()? != 0;
        let start_frame = r.short()?;
        let frame_variation = r.float()?;
        let frame_count = r.short()?;
        let _scroll = [r.float()?, r.float()?];
        let frame_rate = r.float()?;
        // REL14f460 gives seedless systems a CPU_WRITE_STATIC UV buffer.
        // The authored atlas controls cannot animate those persistent UVs.
        if texture_seeds {
            ensure!(
                scrolling == 0,
                "Scrolling nonuniform particle atlas is not implemented"
            );
            ensure!(
                !animated && start_frame == 0,
                "Animated nonuniform particle atlas is not implemented"
            );
        }
        r.done()?;
        Ok(Self {
            center,
            atlas,
            frame_variation,
            animated,
            start_frame,
            frame_count,
            frame_rate,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn velocity_scaling_clamps_factors_before_birth_dimensions() {
        let system = System {
            width: [2., 0.],
            height: [3., 0.],
            ratio: 1.,
            linked: false,
            velocity: Some([4., 8., 0.5, 1., 2., 3.]),
        };
        assert_eq!(system.animated_factors([1., 1.], [0.; 3]), [0.5, 1.]);
        assert_eq!(system.animated_factors([1., 1.], [0., 1., 0.]), [2., 3.]);
        assert_eq!(system.animated_factors([1., 1.], [0., 10., 0.]), [4., 8.]);
    }
    use anyhow::Context;
    #[test]
    fn unseeded_uvs_match_native_initialization_and_point_constants() -> Result<()> {
        let data: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/nonuniform_uv.json"
        ))?;
        let cases = data["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 8);
        for case in cases {
            assert_eq!(
                case["point_constants"],
                serde_json::json!([0, 0, 1f32.to_bits(), 0, 0, 1f32.to_bits()])
            );
            let quads = case["quads"].as_u64().unwrap();
            let expected = if case["seeds"].as_bool().unwrap() {
                vec![3f32.to_bits(); quads as usize * 8]
            } else {
                (0..quads)
                    .flat_map(|_| unseeded_uvs().into_iter().flatten().map(f32::to_bits))
                    .collect()
            };
            assert_eq!(case["uv_bits"], serde_json::json!(expected));
            assert_eq!(
                case["access"].as_u64().unwrap(),
                if case["seeds"].as_bool().unwrap() {
                    0x18
                } else {
                    0x12
                }
            );
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_gremlin_in_preserves_subclass_and_plays_both_particle_layers() -> Result<()> {
        let root = std::path::PathBuf::from(
            std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?,
        );
        let mut files = Vec::new();
        for name in ["_dynamic.pak", "r3mt.pak", "gremlinvillage_zonea.pak"] {
            files.extend(
                crate::pack_io::read(std::fs::read(root.join("packfiles").join(name))?)?
                    .pak
                    .files,
            );
        }
        let file = files
            .iter()
            .find(|f| {
                f.path
                    .eq_ignore_ascii_case("Effects/_Shared/fx_gremlin_warp_in.nif_wii")
            })
            .context("Missing original teleport-in")?;
        let nif = epic_mickey_lib_rs::nif_file::NifFile::load_bytes(&file.data)
            .map_err(anyhow::Error::msg)?;
        let system = nif
            .blocks
            .iter()
            .enumerate()
            .find_map(|(index, block)| {
                if nif.block_types[nif.block_type_indices[index] as usize]
                    != "NonUniformParticleSystem"
                {
                    return None;
                }
                if let epic_mickey_lib_rs::nif_file::NiObject::NiPSParticleSystem(system) = block {
                    Some((index, system))
                } else {
                    None
                }
            })
            .context("Custom particle system lost its typed base")?;
        assert_eq!(system.1.extension.len(), 48);
        let settings = System::load(
            &system.1.extension,
            nif.source_is_little_endian,
            nif.user_version,
        )?;
        assert_eq!(settings.width, [0.75, 0.25]);
        assert_eq!(settings.height, [1.5, -0.5]);
        assert!(!settings.linked);
        for revision in [0u16, 2] {
            let mut legacy = system.1.extension[..23].to_vec();
            legacy[..2].copy_from_slice(&revision.to_be_bytes());
            let legacy = System::load(&legacy, false, nif.user_version)?;
            assert_eq!(legacy.width, settings.width);
        }
        assert!(System::load(
            &system.1.extension[..47],
            nif.source_is_little_endian,
            nif.user_version
        )
        .is_err());
        let mut bytes = Vec::new();
        nif.save_stream(&mut bytes).map_err(anyhow::Error::msg)?;
        let copy = epic_mickey_lib_rs::nif_file::NifFile::load_bytes(&bytes)
            .map_err(anyhow::Error::msg)?;
        let epic_mickey_lib_rs::nif_file::NiObject::NiPSParticleSystem(reloaded) =
            &copy.blocks[system.0]
        else {
            anyhow::bail!("Round-trip changed particle base")
        };
        assert_eq!(reloaded.extension, system.1.extension);
        assert_eq!(copy.block_types, nif.block_types);
        for (index, (source, copy)) in nif.blocks.iter().zip(&copy.blocks).enumerate() {
            use epic_mickey_lib_rs::nif_file::NiObject;
            if let NiObject::NiMesh(source) = source {
                let NiObject::NiMesh(copy) = copy else {
                    anyhow::bail!("Mesh {index} failed serialization")
                };
                assert_eq!(source.em1_fields, copy.em1_fields);
                assert_eq!(source.primitive_type, copy.primitive_type);
            }
            if let NiObject::NiPSParticleSystem(source) = source {
                let NiObject::NiPSParticleSystem(copy) = copy else {
                    anyhow::bail!("Particle mesh {index} failed serialization")
                };
                assert_eq!(source.base.em1_fields, copy.base.em1_fields);
                assert_eq!(source.extension, copy.extension);
            }
        }
        let mut effect = super::super::Hose::load_attached(&file.data, &files)?;
        assert_eq!(effect.layers.len(), 2);
        assert!(effect.layers[1].custom_quad);
        assert_eq!(effect.layers[1].atlas, [2, 2]);
        assert_eq!(effect.layers[1].frame_var, 4.);
        let track = effect.layers[1]
            .curves
            .height
            .as_ref()
            .context("Missing native secondary channel")?;
        assert_eq!(track.sample(0., 2.)?, 1.);
        assert_eq!(track.sample(1., 2.)?, 1.5);
        assert_eq!(track.sample(2., 2.)?, 2.);
        let owner = [
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 10., 20., 30., 1.,
        ];
        let mut seen = [false; 2];
        let mut non_square = false;
        for _ in 0..45 {
            effect.advance_attached(1. / 30., true, owner)?;
            for (index, layer) in effect.layers.iter().enumerate() {
                seen[index] |= !layer.particles.is_empty();
                for particle in &layer.particles {
                    if let Some(height) = particle.height {
                        let width = particle.size / layer.scale;
                        let height = height / layer.scale;
                        assert!((0.5..=1.).contains(&width));
                        assert!((1. ..=2.).contains(&height));
                        non_square |= width != height;
                    }
                }
            }
            let mut draws = Vec::new();
            effect.append([1., 0., 0.], [0., 1., 0.], &mut draws)?;
            assert!(draws.iter().all(|d| d.v_data.iter().all(|v| v.is_finite())));
        }
        assert_eq!(seen, [true, true]);
        assert!(non_square);
        for _ in 0..120 {
            effect.advance_attached(1. / 30., false, owner)?;
        }
        assert!(effect.layers.iter().all(|layer| layer.particles.is_empty()));
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_scene_particle_inspection_finds_scene_local_template() -> Result<()> {
        let root = std::path::PathBuf::from(
            std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?,
        );
        let world = crate::blotworx::assets::load_with_particle_preview(
            &root,
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
            None,
            Some("gremlinvariant4teleportinfx"),
        )?;
        let preview = world
            .particle_asset_preview
            .as_ref()
            .context("Missing live particle asset owner")?;
        assert_eq!(preview.name, "gremlinvariant4teleportinfx");
        let effect = &preview.particles;
        assert_eq!(effect.layers.len(), 2);
        assert!(effect.layers[1].nonuniform.is_some() && effect.layers[1].custom_quad);
        Ok(())
    }
    #[test]
    fn independent_channel_and_birth_sizes_match_original_instructions() -> Result<()> {
        let data: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/particle_nonuniform.json"
        ))?;
        assert_eq!(data["channels"].as_array().unwrap().len(), 120);
        for case in data["channels"].as_array().unwrap() {
            let keys = case["keys"]
                .as_array()
                .unwrap()
                .iter()
                .map(|key| {
                    (
                        key[0].as_f64().unwrap() as f32,
                        key[1].as_f64().unwrap() as f32,
                    )
                })
                .collect();
            let track = Track {
                keys,
                mode: case["mode"].as_u64().unwrap() as u32,
            };
            assert_eq!(
                track
                    .sample(
                        case["age"].as_f64().unwrap() as f32,
                        case["life"].as_f64().unwrap() as f32
                    )?
                    .to_bits(),
                case["bits"].as_u64().unwrap() as u32,
                "{case}"
            );
        }
        assert_eq!(data["births"].as_array().unwrap().len(), 12);
        for case in data["births"].as_array().unwrap() {
            let linked = case["linked"].as_bool().unwrap();
            let system = System {
                width: [0.75, 0.25],
                height: [1.5, -0.5],
                ratio: 1.,
                linked,
                velocity: None,
            };
            let samples = [
                case["first"].as_u64().unwrap() as u32,
                case["second"].as_u64().unwrap() as u32,
            ];
            let mut calls = 0;
            let got = system.sizes(|| {
                let value = samples[calls];
                calls += 1;
                value
            })?;
            assert_eq!(calls, if linked { 1 } else { 2 });
            assert_eq!(
                got.map(f32::to_bits),
                std::array::from_fn::<_, 2, _>(|i| case["bits"][i].as_u64().unwrap() as u32),
                "{case}"
            );
            assert_eq!(case["bits"][2], serde_json::json!(1f32.to_bits()));
        }
        assert_eq!(data["atlas"].as_array().unwrap().len(), 24);
        for case in data["atlas"].as_array().unwrap() {
            let raw = case["raw"].as_u64().unwrap() as u32;
            let atlas = std::array::from_fn(|i| case["atlas"][i].as_u64().unwrap() as u16);
            assert_eq!(
                atlas_state(raw)?.to_bits(),
                case["state_bits"].as_u64().unwrap() as u32
            );
            assert_eq!(
                atlas_frame(raw, atlas)?,
                case["frame"].as_u64().unwrap() as u32
            );
        }
        Ok(())
    }
}
