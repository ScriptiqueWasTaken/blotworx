//! Material source selection used by REL 0x25b158 -> 0x25b6f8.
use crate::render_support::{material_preview, viewport3d::DecodedImage};
use crate::render_support::{
    nif::{MaterialBindings, MaterialTexture},
    viewport3d::TextureSampler,
};
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::nif_file::{NiObject, NifFile};
use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};
pub struct Prepared {
    pub program: std::sync::Arc<material_preview::Program>,
    pub images: Vec<material_preview::MaterialImage>,
}

pub struct Template {
    /// Preserve properties/controllers for subsequent clone and animation.
    pub nif: NifFile,
    pub mesh: Option<usize>,
    pub bindings: MaterialBindings,
    pub colors: BTreeMap<String, [f32; 4]>,
    pub integers: BTreeMap<String, i32>,
    /// Shader/texture preparation only. Native property overrides and animated
    /// material state must still be applied before this becomes a draw command.
    pub rendering: Option<std::result::Result<Prepared, String>>,
}
impl Template {
    pub fn load(bytes: &[u8]) -> Result<Self> {
        let nif = NifFile::load_bytes(bytes)
            .map_err(anyhow::Error::msg)
            .context("Invalid decal material NIF")?;
        let root = nif.roots.first().copied().flatten();
        let path = match root {
            Some(root) => first_mesh(&nif, root as usize, &mut HashSet::new())?,
            None => None,
        };
        let mesh = path.as_ref().and_then(|path| path.last().copied());
        let mut template = Self {
            nif,
            mesh,
            bindings: MaterialBindings::default(),
            rendering: None,
            colors: BTreeMap::new(),
            integers: BTreeMap::new(),
        };
        template.bindings.shader = template.shader().map(str::to_owned);
        if let Some(mesh) = mesh {
            let links = match &template.nif.blocks[mesh] {
                NiObject::NiMesh(m) => &m.base.base.base.extra_data_links,
                NiObject::NiPSParticleSystem(p) => &p.base.base.base.base.extra_data_links,
                _ => unreachable!(),
            };
            for link in links.iter().flatten() {
                match template
                    .nif
                    .blocks
                    .get(*link as usize)
                    .context("Invalid decal extra-data link")?
                {
                    NiObject::NiIntegerExtraData(data) => {
                        template.integers.insert(data.base.name.clone(), data.value);
                    }
                    NiObject::NiBooleanExtraData(data) if data.base.name == "R3MTStateOverride" => {
                        template
                            .integers
                            .insert(data.base.name.clone(), i32::from(data.value != 0));
                    }
                    NiObject::Unknown(raw) if raw.type_name == "NiColorExtraData" => {
                        ensure!(
                            template.nif.version >= 0x14010001 && raw.raw_data.len() == 20,
                            "Unsupported decal color extra-data layout"
                        );
                        let word = |at: usize| {
                            let bytes = raw.raw_data[at..at + 4].try_into().unwrap();
                            if template.nif.source_is_little_endian {
                                u32::from_le_bytes(bytes)
                            } else {
                                u32::from_be_bytes(bytes)
                            }
                        };
                        let name = template
                            .nif
                            .string_table
                            .get(word(0) as usize)
                            .context("Invalid decal color name")?;
                        let value: [f32; 4] =
                            std::array::from_fn(|i| f32::from_bits(word(4 + i * 4)));
                        ensure!(
                            value.iter().all(|v| v.is_finite()),
                            "Non-finite decal color {name}"
                        );
                        template.colors.insert(name.clone(), value);
                    }
                    _ => {}
                }
            }
        }
        if let Some(path) = path {
            // Effective nearest texture property on the selected traversal path.
            for index in path.into_iter().rev() {
                let links = match &template.nif.blocks[index] {
                    NiObject::NiNode(n) => &n.base.property_links,
                    NiObject::NiSortAdjustNode(n) => &n.base.base.property_links,
                    NiObject::NiMesh(m) => &m.base.base.property_links,
                    NiObject::NiPSParticleSystem(p) => &p.base.base.base.property_links,
                    _ => continue,
                };
                let mut found = false;
                for link in links.iter().flatten() {
                    let property = template
                        .nif
                        .blocks
                        .get(*link as usize)
                        .context("Invalid decal property link")?;
                    if let NiObject::Unknown(raw) = property {
                        if raw.type_name != "NiTexturingProperty" {
                            continue;
                        }
                        found = true;
                        let little = template.nif.source_is_little_endian;
                        for input in crate::nif_textures::bindings(
                            &raw.raw_data,
                            little,
                            template.nif.version,
                        )? {
                            let NiObject::NiSourceTexture(texture) = template
                                .nif
                                .blocks
                                .get(input.source as usize)
                                .context("Invalid decal texture source link")?
                            else {
                                anyhow::bail!("Decal binding is not a texture source");
                            };
                            let path = texture
                                .file_name
                                .as_ref()
                                .filter(|p| !p.is_empty())
                                .context("Embedded decal texture sources are not implemented")?
                                .clone();
                            let at = input.source_offset + 4;
                            let bytes: [u8; 2] = raw
                                .raw_data
                                .get(at..at + 2)
                                .context("Truncated decal texture flags")?
                                .try_into()?;
                            let flags = if little {
                                u16::from_le_bytes(bytes)
                            } else {
                                u16::from_be_bytes(bytes)
                            };
                            template.bindings.textures.push(MaterialTexture {
                                shader_id: input.shader_id,
                                path,
                                uv_set: (flags & 255) as u8,
                                slot: input.slot as u32,
                                sampler: TextureSampler::from_nif_flags(flags),
                                transform: crate::nif_textures::transform(
                                    &raw.raw_data,
                                    &input,
                                    little,
                                    template.nif.version,
                                )?,
                            });
                        }
                        break;
                    }
                }
                if found {
                    break;
                }
            }
        }
        template.bindings.integers = template.integers.clone();
        Ok(template)
    }
    pub fn prepare(
        &mut self,
        sources: &material_preview::Sources,
        files: &[epic_mickey_lib_rs::packfile::VirtualFile],
        textures: &mut std::collections::HashMap<String, Option<DecodedImage>>,
    ) {
        let program = if self.colors.is_empty() {
            material_preview::prepare(&self.bindings, sources, false)
        } else {
            (|| -> Result<_> {
                let name = self.shader().context("No decal shader")?;
                let source = files
                    .iter()
                    .rev()
                    .find(|f| {
                        let path = f.path.replace('\\', "/");
                        path.rsplit('/')
                            .next()
                            .is_some_and(|file| file.eq_ignore_ascii_case(&format!("{name}.r3mt")))
                    })
                    .context("Missing decal shader source")?;
                let registers = self.registers()?;
                Ok(std::sync::Arc::new(
                    material_preview::compile_uniform_registers(
                        std::str::from_utf8(&source.data)?,
                        false,
                        &registers,
                    )?,
                ))
            })()
            .map_err(|error| format!("{error:#}"))
        };
        self.rendering = Some(program.and_then(|program| {
            let images = material_preview::images(&program, &self.bindings, files, textures)?;
            Ok(Prepared { program, images })
        }));
    }
    /// Build a command from projected geometry, without using the material
    /// NIF's placeholder mesh or its Maya-to-engine transform.
    pub fn draw(
        &self,
        geometry: super::decal_geometry::Projected,
        transform: [f32; 16],
    ) -> Result<crate::render_support::viewport3d::GpuDrawCommand> {
        let flags = self
            .integers
            .get("R3MTStateOverrideFlags")
            .copied()
            .unwrap_or_else(|| {
                self.integers
                    .get("R3MTStateOverride")
                    .map_or(7, |value| if *value != 0 { 7 } else { 0 })
            });
        // 0x179c00: mask 7 selects the shader's complete PE state. Clear bits
        // select NiProperty state instead; that path must not silently use XML.
        ensure!(
            flags == 7,
            "Decal NiProperty render-state fallback {flags:#x} is not implemented"
        );
        ensure!(
            !self
                .nif
                .blocks
                .iter()
                .any(|block| block.type_name().contains("Controller")
                    || block.type_name().contains("Interpolator")),
            "Animated decal materials are not implemented"
        );
        let prepared = self
            .rendering
            .as_ref()
            .context("Decal material has not been prepared")?
            .as_ref()
            .map_err(|e| anyhow::anyhow!("Decal material: {e}"))?;
        ensure!(
            prepared
                .program
                .texture_slots
                .iter()
                .all(|slot| prepared.program.uv_textures.contains(slot)),
            "Decal generated-coordinate inputs are not implemented"
        );
        ensure!(
            !geometry.positions.is_empty() && geometry.positions.len() == geometry.uv.len(),
            "Invalid projected decal attributes"
        );
        ensure!(
            geometry
                .indices
                .iter()
                .all(|i| (*i as usize) < geometry.positions.len()),
            "Invalid projected decal indices"
        );
        ensure!(
            transform
                .iter()
                .chain(geometry.positions.iter().flatten())
                .chain(geometry.uv.iter().flatten())
                .all(|v| v.is_finite()),
            "Non-finite projected decal"
        );
        let mut vertices = Vec::with_capacity(geometry.positions.len() * 12);
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for (p, uv) in geometry.positions.iter().zip(&geometry.uv) {
            for i in 0..3 {
                min[i] = min[i].min(p[i]);
                max[i] = max[i].max(p[i]);
            }
            // Original generated streams contain position and UV only. The
            // shared vertex layout needs unused normal/color attributes.
            vertices.extend_from_slice(p);
            vertices.extend_from_slice(&[0.; 3]);
            vertices.extend_from_slice(uv);
            vertices.extend_from_slice(&[1.; 4]);
        }
        Ok(crate::render_support::viewport3d::GpuDrawCommand {
            shader_time:None,source_attributes: None,
            extra_uvs: None,
            projection_bound: None,
            projected_shadow: false,
            skybox: false,
            material_registers: None,
            material_indirect: None,
            material_images: Arc::new(prepared.images.clone()),
            baked: false,
            baked_error: None,
            preview: Some(Ok(prepared.program.clone())),
            uv_transform: None,
            material: Some(std::sync::Arc::new(self.bindings.clone())),
            sampler: TextureSampler::default(),
            v_data: std::sync::Arc::new(vertices),
            indices: std::sync::Arc::new(geometry.indices.into_iter().map(u32::from).collect()),
            trans_mat: transform,
            color: [1.; 4],
            tex_bytes: None,
            aabb_min: min,
            aabb_max: max,
            light_group: Arc::from(""),
        })
    }
    fn registers(&self) -> Result<crate::tev::Registers> {
        let mut registers = crate::tev::Registers::default();
        for (name, value) in &self.colors {
            let slot = if let Some(index) = name.strip_prefix("Color_Register_") {
                registers
                    .colors
                    .get_mut(index.parse::<usize>()?)
                    .context("Invalid color register")?
            } else if let Some(index) = name.strip_prefix("Constant_Color_") {
                registers
                    .constants
                    .get_mut(index.parse::<usize>()?)
                    .context("Invalid constant register")?
            } else {
                anyhow::bail!("Unsupported decal color parameter {name}");
            };
            let mut bytes = [0; 4];
            for (i, value) in value.iter().enumerate() {
                let scaled = 255_f32 * value;
                ensure!(
                    scaled.is_finite() && scaled >= -2147483648. && scaled < 2147483648.,
                    "Decal color {name} exceeds verified integer conversion range"
                );
                // REL 0x170318..0x17037c: fmuls, fctiwz, stb. Truncate
                // toward zero, then retain the low byte; do not clamp/round.
                bytes[i] = (scaled as i32) as u8;
            }
            *slot = Some(bytes);
        }
        Ok(registers)
    }
    pub fn shader(&self) -> Option<&str> {
        let mesh = self.mesh?;
        let base = match &self.nif.blocks[mesh] {
            NiObject::NiMesh(m) => &m.base,
            NiObject::NiPSParticleSystem(p) => &p.base.base,
            _ => return None,
        };
        base.materials
            .get(usize::try_from(base.active_material).ok()?)
            .map(|m| m.name.as_str())
    }
}
fn first_mesh(
    nif: &NifFile,
    root: usize,
    ancestors: &mut HashSet<usize>,
) -> Result<Option<Vec<usize>>> {
    let block = nif
        .blocks
        .get(root)
        .context("Invalid decal material node link")?;
    if !matches!(block, NiObject::NiNode(_) | NiObject::NiSortAdjustNode(_)) {
        return Ok(None);
    }
    ensure!(
        ancestors.insert(root),
        "Cycle in decal material node hierarchy"
    );
    for child in block.children().iter().flatten() {
        let child = *child as usize;
        let block = nif
            .blocks
            .get(child)
            .context("Invalid decal material child link")?;
        if matches!(block, NiObject::NiMesh(_) | NiObject::NiPSParticleSystem(_)) {
            ancestors.remove(&root);
            return Ok(Some(vec![root, child]));
        }
        if let Some(mut path) = first_mesh(nif, child, ancestors)? {
            path.insert(0, root);
            ancestors.remove(&root);
            return Ok(Some(path));
        }
    }
    ancestors.remove(&root);
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn register_conversion_truncates_then_wraps_low_byte() -> Result<()> {
        let template = Template {
            nif: NifFile::default(),
            mesh: None,
            bindings: MaterialBindings::default(),
            colors: BTreeMap::from([("Color_Register_1".into(), [0.5, -0.5, 2., 1.])]),
            integers: BTreeMap::new(),
            rendering: None,
        };
        let registers = template.registers()?;
        assert_eq!(registers.colors, [None, Some([127, 129, 254, 255]), None]);
        assert_eq!(registers.constants, [None; 4]);
        Ok(())
    }
}
