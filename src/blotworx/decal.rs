//! DecalComponent request preparation, retail REL 0x316c28..0x316df0.
//! Projecting the selected material onto target geometry remains separate.
use anyhow::{ensure, Context, Result};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};

pub struct Component {
    pub files: Vec<String>,
    pub radii: Vec<f32>,
    pub lifetimes: Vec<f32>,
    /// One copy per source path, retaining the component's original indices.
    pub sources: BTreeMap<String, Arc<Vec<u8>>>,
    pub templates: BTreeMap<String, super::decal_material::Template>,
}
#[derive(Debug, PartialEq)]
pub struct Request<'a> {
    pub material_index: usize,
    pub material: &'a str,
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub radius: f32,
    pub lifetime: f32,
}
impl Component {
    pub fn load(
        properties: &[Value],
        mut read: impl FnMut(&str) -> Result<Vec<u8>>,
    ) -> Result<Self> {
        let array = |name: &str| -> Result<&Vec<Value>> {
            let mut found = properties.iter().filter(|p| p["name"] == name);
            let p = found
                .next()
                .with_context(|| format!("Missing decal {name}"))?;
            ensure!(found.next().is_none(), "Duplicate decal {name}");
            p["value"]
                .as_array()
                .with_context(|| format!("Invalid decal {name}"))
        };
        let floats = |name: &str| -> Result<Vec<f32>> {
            array(name)?
                .iter()
                .map(|v| {
                    let n = v.as_f64().context("Invalid decal number")? as f32;
                    ensure!(n.is_finite(), "Non-finite decal {name}");
                    Ok(n)
                })
                .collect()
        };
        let files = array("DecalMaterialNifFile")?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .context("Invalid decal material path")
            })
            .collect::<Result<Vec<_>>>()?;
        let mut sources = BTreeMap::new();
        for path in &files {
            if !sources.contains_key(path) {
                sources.insert(
                    path.clone(),
                    Arc::new(read(path).with_context(|| format!("Decal material {path}"))?),
                );
            }
        }
        Ok(Self {
            files,
            radii: floats("DecalRadius")?,
            lifetimes: floats("DecalLifetime")?,
            sources,
            templates: BTreeMap::new(),
        })
    }
    pub fn prepare_materials(&mut self) -> Result<()> {
        let templates = self
            .sources
            .iter()
            .map(|(path, bytes)| {
                super::decal_material::Template::load(bytes)
                    .with_context(|| format!("Decal material {path}"))
                    .map(|template| (path.clone(), template))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        self.templates = templates;
        Ok(())
    }
    /// -1 selects rand()%authored_count, consuming RNG even for a single
    /// material. Other invalid indices return no request without consuming it.
    /// Normal is passed through; retail does not normalize it here.
    pub fn request(
        &self,
        index: i32,
        position: [f32; 3],
        normal: [f32; 3],
        random: impl FnOnce() -> u32,
    ) -> Option<Request<'_>> {
        let index = if index == -1 && !self.files.is_empty() {
            (random() as usize) % self.files.len()
        } else {
            usize::try_from(index).ok()?
        };
        let material = self.files.get(index)?;
        // Native defaults before the indexed property getters: 1 and 5.
        let radius = self.radii.get(index).copied().unwrap_or(1.);
        let lifetime = self.lifetimes.get(index).copied().unwrap_or(5.);
        // Exact constants at REL 0x4eea94/98. Above 2.5, offset scales with
        // radius; otherwise it remains 0.05, including the boundary itself.
        let offset = if !(radius <= 2.5) {
            radius * 0.1_f32
        } else {
            0.05_f32
        };
        Some(Request {
            material_index: index,
            material,
            position: std::array::from_fn(|i| offset.mul_add(normal[i], position[i])),
            normal,
            radius,
            lifetime,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_consumes_original_rng_and_keeps_authored_indices() -> Result<()> {
        let properties = serde_json::json!([
            {"name":"DecalMaterialNifFile","value":["paint.nif","paint.nif","thinner.nif"]},
            {"name":"DecalRadius","value":[2.]},
            {"name":"DecalLifetime","value":[5.]}
        ]);
        let mut loads = Vec::new();
        let c = Component::load(properties.as_array().unwrap(), |path| {
            loads.push(path.to_owned());
            Ok(vec![1])
        })?;
        assert_eq!(loads, ["paint.nif", "thinner.nif"]);
        let mut draws = 0;
        let request = c
            .request(-1, [0.; 3], [0., 1., 0.], || {
                draws += 1;
                5
            })
            .unwrap();
        assert_eq!(draws, 1);
        assert_eq!(request.material_index, 2);
        assert_eq!(request.material, "thinner.nif");
        assert_eq!((request.radius, request.lifetime), (1., 5.));
        assert_eq!(request.position, [0., 0.05, 0.]);
        for index in [-2, 3, i32::MAX] {
            assert!(c
                .request(index, [0.; 3], [0.; 3], || panic!(
                    "invalid index consumed RNG"
                ))
                .is_none());
        }
        Ok(())
    }
    #[test]
    fn placement_uses_threshold_and_supplied_normal_without_renormalization() {
        let mut c = Component {
            files: vec!["paint.nif".into()],
            radii: vec![2.5],
            lifetimes: vec![],
            sources: BTreeMap::new(),
            templates: BTreeMap::new(),
        };
        let r = c
            .request(0, [1., 2., 3.], [0., 2., 0.], || {
                panic!("explicit index consumed RNG")
            })
            .unwrap();
        assert_eq!(r.position, [1., 2.1, 3.]);
        c.radii[0] = 3.;
        let mut draws = 0;
        let r = c
            .request(-1, [1., 2., 3.], [0., 2., 0.], || {
                draws += 1;
                123
            })
            .unwrap();
        assert_eq!(draws, 1);
        assert_eq!(r.position, [1., 2.6, 3.]);
        c.files.clear();
        assert!(c
            .request(-1, [0.; 3], [0.; 3], || panic!(
                "empty component consumed RNG"
            ))
            .is_none());
    }
}
