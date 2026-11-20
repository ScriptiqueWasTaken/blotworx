//! Bound Gamebryo material-color and visibility pose channels.
//! Layout and bool blending follow the 2.6 evaluator/pose-handler sources.
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::nif_file::{NiObject, NifFile};
#[derive(Debug)]
pub enum Value {
    Color(super::nif_transform::Track),
    ConstColor([f32; 4]),
    Visibility(f32),
    VisibilityKeys(BoolKeys),
}
/// NiBoolData's portable time/byte step keys (NiBoolKey::STEPKEY=5).
/// Evaluation follows NiBoolKey::GenCubicCoefs: the new value owns the exact
/// key time, including the final key. No float interpolation or key skipping.
#[derive(Debug)]
pub struct BoolKeys(Vec<(f32,bool)>);
impl BoolKeys {
    fn load(nif:&NifFile,id:u32)->Result<Self> {
        let Some(NiObject::Unknown(raw))=nif.blocks.get(id as usize) else {anyhow::bail!("Missing raw NiBoolData");};
        ensure!(raw.type_name=="NiBoolData","Visibility references non-bool data");
        let bytes=&raw.raw_data;
        let word=|at:usize|->Result<u32> {
            let b=bytes.get(at..at+4).context("Truncated bool key data")?.try_into().unwrap();
            Ok(if nif.source_is_little_endian {u32::from_le_bytes(b)} else {u32::from_be_bytes(b)})
        };
        let count=usize::try_from(word(0)?)?;
        if count==0 {ensure!(bytes.len()==4,"Trailing empty bool data");return Ok(Self(Vec::new()));}
        ensure!(word(4)?==5,"Unsupported bool key interpolation");
        ensure!(count.checked_mul(5).and_then(|n|n.checked_add(8))==Some(bytes.len()),"Invalid bool key data size");
        let mut keys=Vec::with_capacity(count);
        for i in 0..count {
            let time=f32::from_bits(word(8+i*5)?);
            ensure!(time.is_finite() && keys.last().is_none_or(|&(old,_)|old<time),"Invalid/unordered bool key times");
            keys.push((time,bytes[12+i*5]!=0));
        }
        Ok(Self(keys))
    }
    pub fn sample(&self,time:f32)->Result<Option<f32>> {
        ensure!(time.is_finite(),"Nonfinite bool evaluator time");
        let Some(first)=self.0.first() else {return Ok(None)};
        let count=self.0.partition_point(|&(key,_)|key<=time);
        let value=if count==0 {first.1} else {self.0[count-1].1};
        Ok(Some(if value {1.}else{0.}))
    }
}
#[derive(Debug)]
pub struct Channel {
    pub target: u32,
    pub slot: Option<usize>,
    pub value: Value,
}
impl Channel {
    pub fn load(
        nif: &NifFile,
        id: u32,
        mut resolve: impl FnMut(&str) -> Result<Option<u32>>,
    ) -> Result<Option<Self>> {
        let block = nif
            .blocks
            .get(id as usize)
            .context("Missing property evaluator")?;
        if !matches!(
            block.type_name(),
            "NiColorEvaluator" | "NiConstColorEvaluator" | "NiConstBoolEvaluator" | "NiBoolEvaluator"
        ) {
            return Ok(None);
        }
        let NiObject::Unknown(raw) = block else {
            anyhow::bail!("Expected raw property evaluator")
        };
        ensure!(
            nif.version == 0x14060500,
            "Unverified property evaluator version"
        );
        ensure!(
            raw.raw_data.len()
                == if raw.type_name == "NiConstColorEvaluator" {
                    40
                } else {
                    28
                },
            "Invalid property evaluator layout"
        );
        let word = |at| {
            let b = raw.raw_data[at..at + 4].try_into().unwrap();
            if nif.source_is_little_endian {
                u32::from_le_bytes(b)
            } else {
                u32::from_be_bytes(b)
            }
        };
        let string = |at| -> Result<Option<&str>> {
            let index = word(at);
            if index == u32::MAX {
                Ok(None)
            } else {
                Ok(Some(
                    nif.string_table
                        .get(index as usize)
                        .context("Missing property binding string")?
                        .as_str(),
                ))
            }
        };
        // Other bool consumers (notably particle emitter bindings) still use
        // their own handlers. Leave them in ActorBindings' explicit inventory.
        if raw.type_name=="NiBoolEvaluator" && string(8)?!=Some("NiVisController") {return Ok(None);}
        let object = string(0)?.context("Missing property target name")?;
        ensure!(
            string(4)?.is_none() && string(16)?.is_none(),
            "Qualified property evaluator requires binding resolution"
        );
        let target =
            resolve(object)?.with_context(|| format!("Missing property target {object}"))?;
        let (slot, value) = if matches!(raw.type_name.as_str(),"NiConstBoolEvaluator" | "NiBoolEvaluator") {
            ensure!(
                string(8)? == Some("NiVisController") && string(12)?.is_none(),
                "Unsupported bool controller binding"
            );
            if raw.type_name=="NiBoolEvaluator" {
                ensure!(matches!(&raw.raw_data[20..24],[2,0,0,0]|[0x42,0,0,0]),"Unsupported keyed visibility evaluator flags");
                (None,Value::VisibilityKeys(BoolKeys::load(nif,word(24))?))
            } else {
                ensure!(raw.raw_data[20..24]==[0x42,0,0,0],"Unsupported visibility evaluator flags");
                let value=f32::from_bits(word(24));
                ensure!(value.is_finite(),"Nonfinite visibility");
                (None,Value::Visibility(value))
            }
        } else {
            ensure!(
                string(8)? == Some("NiColorExtraDataController"),
                "Unsupported color controller binding"
            );
            let name = string(12)?.context("Missing color register name")?;
            let slot = if let Some(s) = name.strip_prefix("Constant_Color_") {
                let s = s.parse::<usize>()?;
                ensure!(s < 4, "Invalid constant register");
                3 + s
            } else if let Some(s) = name.strip_prefix("Color_Register_") {
                let s = s.parse::<usize>()?;
                ensure!(s < 3, "Invalid color register");
                s
            } else {
                anyhow::bail!("Unsupported animated extra data {name}")
            };
            let value = if raw.type_name == "NiColorEvaluator" {
                ensure!(
                    raw.raw_data[20..24] == [1, 0, 0, 0],
                    "Unsupported color evaluator flags"
                );
                Value::Color(super::nif_transform::Track::load_color_data(nif, word(24))?)
            } else {
                ensure!(
                    raw.raw_data[20..24] == [0x41, 0, 0, 0],
                    "Unsupported constant color evaluator flags"
                );
                let color = std::array::from_fn(|i| f32::from_bits(word(24 + 4 * i)));
                ensure!(
                    color.iter().all(|v| v.is_finite()),
                    "Nonfinite constant color"
                );
                Value::ConstColor(color)
            };
            (Some(slot), value)
        };
        Ok(Some(Self {
            target,
            slot,
            value,
        }))
    }
}
#[derive(Clone, Debug, serde::Serialize)]
pub(super) struct DirectColor {
    pub slot: usize,
    pub track: Option<super::nif_transform::Track>,
    pub value: [f32; 4],
}

impl DirectColor {
    pub(super) fn load(nif: &NifFile, name: u32, interpolator: u32) -> Result<Option<Self>> {
        let Some(name) = nif.string_table.get(name as usize) else { return Ok(None); };
        let slot = if let Some(index) = name.strip_prefix("Constant_Color_") {
            index.parse::<usize>().ok().filter(|&i| i < 4).map(|i| i + 3)
        } else if let Some(index) = name.strip_prefix("Color_Register_") {
            index.parse::<usize>().ok().filter(|&i| i < 3)
        } else {
            None
        };
        let Some(slot) = slot else { return Ok(None); };
        let Some(NiObject::Unknown(raw)) = nif.blocks.get(interpolator as usize) else {
            anyhow::bail!("Color controller is missing its interpolator");
        };
        ensure!(raw.type_name == "NiColorInterpolator" && raw.raw_data.len() == 20,
            "Unsupported direct color interpolator");
        let word = |offset| {
            let bytes = raw.raw_data[offset..offset + 4].try_into().unwrap();
            if nif.source_is_little_endian { u32::from_le_bytes(bytes) } else { u32::from_be_bytes(bytes) }
        };
        let data = word(16);
        let track = (data != u32::MAX).then(|| super::nif_transform::Track::load_color_data(nif, data)).transpose()?;
        let value = std::array::from_fn(|i| f32::from_bits(word(i * 4)));
        Ok(Some(Self { slot, track, value }))
    }

    pub(super) fn sample(&self, time: f32) -> Result<Option<[f32; 4]>> {
        if let Some(track) = &self.track {
            track.sample_vector::<4>(time)
        } else {
            Ok((self.value[0] != -f32::MAX).then_some(self.value))
        }
    }
}

pub fn packed_color(color: [f32; 4]) -> Result<[f32; 4]> {
    let mut result = [0.; 4];
    for (out, value) in result.iter_mut().zip(color) {
        let value = value * 255.;
        ensure!(
            value.is_finite() && (-2147483648.0..2147483648.0).contains(&(value as f64)),
            "Invalid material color"
        );
        *out = f32::from((value as i32) as u8) / 255.;
    }
    Ok(result)
}

#[cfg(test)]
mod visibility_tests {
    use super::*;
    #[test]
    fn step_visibility_owns_exact_key_times_and_supports_reverse_sampling()->Result<()> {
        let keys=BoolKeys(vec![(0.,true),(0.5,false),(1.,true),(1.7,false)]);
        for (time,visible) in [(-1.,1.),(0.,1.),(0.49999997,1.),(0.5,0.),(0.9,0.),(1.,1.),(1.6999999,1.),(1.7,0.),(3.,0.),(0.25,1.)] {
            assert_eq!(keys.sample(time)?,Some(visible),"time {time}");
        }
        assert_eq!(BoolKeys(vec![]).sample(0.)?,None);
        assert_eq!(BoolKeys(vec![(2.,false)]).sample(-100.)?,Some(0.));
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_pipe_spawn_keyed_visibility_removes_descendant_geometry_at_end()->Result<()> {
        let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world=super::super::assets::load(std::path::Path::new(&root),"Levels/GV_ZoneA_Start.level",&egui::Context::default())?;
        let entity=world.scripts.resolve_name("gv_zonea_airlock.spatterpipespawner 01.PipeSpawner_Spatter 01").context("Original pipe spawn actor missing")?;
        let resources=&world.actor_sequences[&entity];
        let bindings=&world.actor_bindings[&entity];
        ensure!(!bindings.other_evaluators.contains("NiBoolEvaluator"),"Visibility still classified as unsupported");
        let duration=resources.get(0).context("Original Spawn sequence missing")?.0.duration;
        let start=bindings.sample(0,0.)?;
        let before_end=bindings.sample(0,f32::from_bits(duration.to_bits()-1))?;
        let end=bindings.sample(0,duration)?;
        ensure!(start.visibility.len()==1 && start.visibility.values().all(|&v|v==1.),"Spawn does not start visible");
        ensure!(before_end.visibility.values().all(|&v|v==1.) && end.visibility.values().all(|&v|v==0.),"Authored final key did not switch visibility");
        let actor=world.actors.get_mut(&entity).context("Original spawn actor not initialized")?;
        actor.request(0,resources,bindings)?;
        actor.update(resources,bindings,0.,0.)?;
        let indices=world.entities[entity].mesh_draws.values().flatten().copied().collect::<Vec<_>>();
        ensure!(!indices.is_empty(),"Original spawn mesh has no draws");
        let mut visible=world.draws.clone();world.apply_model_poses(&mut visible)?;
        ensure!(indices.iter().any(|&i|!visible[i].indices.is_empty()),"Start pose did not render");
        world.actors.get_mut(&entity).unwrap().update(&world.actor_sequences[&entity],&world.actor_bindings[&entity],duration+1.,duration+1.)?;
        let mut hidden=world.draws.clone();world.apply_model_poses(&mut hidden)?;
        ensure!(indices.iter().all(|&i|hidden[i].indices.is_empty()),"Completed parent visibility did not remove its mesh descendants");
        ensure!(indices.iter().any(|&i|!world.draws[i].indices.is_empty()),"Pose application modified immutable source draws");
        Ok(())
    }
}

#[cfg(debug_assertions)]
pub fn validate_original_portal() -> Result<()> {
    let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
    let root = super::assets::files_root(std::path::Path::new(&root));
    let archive = std::fs::read_dir(root.join("packfiles"))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| {
            p.file_name().is_some_and(|n| {
                n.to_string_lossy()
                    .eq_ignore_ascii_case("meanstreet_visit1_wrapper.pak")
            })
        })
        .context("Missing portal archive")?;
    let files = crate::pack_io::read(std::fs::read(archive)?)?.pak.files;
    let stem = "Environments/ThePark/MeanStreetUSA/Props/Misc/MSUSA_PortalPowerMach";
    let file = files
        .iter()
        .find(|f| {
            f.path
                .replace('\\', "/")
                .eq_ignore_ascii_case(&format!("{stem}.nif_wii"))
        })
        .context("Missing original portal model")?;
    let model = NifFile::load_bytes(&file.data).map_err(anyhow::Error::msg)?;
    let hierarchy = super::scene_bounds::Hierarchy::load(&model)?;
    let root = hierarchy
        .roots
        .iter()
        .flatten()
        .copied()
        .next()
        .context("Missing portal root")?;
    let resources =
        super::nif_sequence::SequenceResources::load_from_assets(&format!("{stem}.kfm"), &files)?;
    let bindings =
        super::nif_sequence::ActorBindings::prepare(&resources, &model, &hierarchy, root)?;
    let duration = resources.get(1).context("Missing On sequence")?.0.duration;
    let off = bindings.sample(0, 0.)?;
    let start = bindings.sample(1, 0.)?;
    let end = bindings.sample(1, duration)?;
    ensure!(
        off.colors.len() == 1 && start.colors.len() == 1 && end.colors.len() == 1,
        "Portal color channel missing"
    );
    let (&key, &first) = start.colors.first_key_value().unwrap();
    let last = end.colors[&key];
    ensure!(
        key.1 == 4 && first[0] < 0.4 && last == [1., 1., 1., 0.],
        "Original portal light color did not animate"
    );
    ensure!(
        super::material_animation::model_registers(&model)?.contains_key(&key.0),
        "Animated portal color has no mesh binding"
    );
    std::fs::write(
        "target/demo-original-portal-animation.json",
        serde_json::to_vec_pretty(
            &serde_json::json!({"node":key.0,"slot":key.1,"start":first,"end":last,"duration":duration}),
        )?,
    )?;
    println!("PASS: original Mean Street portal material color animation");
    Ok(())
}
