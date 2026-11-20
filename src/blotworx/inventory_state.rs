//! Authored inventory only; runtime setters, save restoration and events pending.
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::scene_file::Component;
use std::collections::BTreeMap;

#[derive(Clone, Debug, serde::Serialize, PartialEq)]
pub struct Authored {
    pub floats: BTreeMap<String, f32>,
    pub sketches: Vec<String>,
    pub counts: Vec<i32>,
    pub maxima: Vec<f32>,
}
impl Authored {
    /// Query authored values with an explicit locale; does not initialize or
    /// mutate runtime inventory. Amount must already be converted natively.
    pub fn preview_add(&self, item: &str, amount: f32, locale: &[u8; 128]) -> Result<bool> {
        let item = c_name(item);
        let mut query = Query {
            state: self,
            locale,
            names: BTreeMap::new(),
        };
        for name in super::inventory_commands::POWERUP_NAMES {
            query.intern(name);
        }
        for name in &self.sketches {
            query.intern(c_name(name));
        }
        let handle = query.intern(item);
        super::inventory_commands::preview_add(item, handle, amount, &mut query)
    }
    pub fn from_component(component: &Component) -> Result<Self> {
        ensure!(
            component.class_name == "InventoryComponent",
            "Expected InventoryComponent"
        );
        let field = |name: &str| -> Result<&serde_json::Value> {
            let mut matches = component.properties.iter().filter(|p| p.name == name);
            let p = matches
                .next()
                .with_context(|| format!("Inventory {name} missing; default unverified"))?;
            ensure!(
                matches.next().is_none(),
                "Duplicate inventory property {name}"
            );
            Ok(&p.value)
        };
        let array = |name: &str| -> Result<&Vec<serde_json::Value>> {
            field(name)?
                .as_array()
                .with_context(|| format!("Inventory {name} must be an array"))
        };
        let float = |value: &serde_json::Value, name: &str| -> Result<f32> {
            let value = value
                .as_f64()
                .with_context(|| format!("Inventory {name} must be float"))?
                as f32;
            ensure!(value.is_finite(), "Nonfinite authored inventory {name}");
            Ok(value)
        };
        let sketches = array("Sketch")?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .context("Sketch entry must be string")
            })
            .collect::<Result<Vec<_>>>()?;
        let counts = array("SketchCount")?
            .iter()
            .map(|v| {
                let n = v.as_i64().context("SketchCount entry must be integer")?;
                i32::try_from(n).context("SketchCount outside signed 32-bit range")
            })
            .collect::<Result<Vec<_>>>()?;
        let maxima = array("SketchMax")?
            .iter()
            .map(|v| float(v, "SketchMax"))
            .collect::<Result<Vec<_>>>()?;
        // Preserve the arrays independently: never zip, truncate, sort or dedup.
        let mut floats = BTreeMap::new();
        for property in &component.properties {
            if ["Sketch", "SketchCount", "SketchMax"].contains(&property.name.as_str()) {
                continue;
            }
            ensure!(
                property.class_name == "Float",
                "Unsupported inventory property {} ({})",
                property.name,
                property.class_name
            );
            ensure!(
                floats
                    .insert(
                        property.name.clone(),
                        float(&property.value, &property.name)?
                    )
                    .is_none(),
                "Duplicate inventory property {}",
                property.name
            );
        }
        Ok(Self {
            floats,
            sketches,
            counts,
            maxima,
        })
    }
    pub fn powerup_values(&self) -> Result<[f32; 4]> {
        let mut values = [0.; 4];
        for (index, name) in super::inventory_commands::POWERUP_NAMES.iter().enumerate() {
            values[index] = *self
                .floats
                .get(*name)
                .with_context(|| format!("Inventory {name} missing; default unverified"))?;
        }
        Ok(values)
    }
}
fn c_name(name: &str) -> &str {
    name.split('\0').next().unwrap_or("")
}
struct Query<'a> {
    state: &'a Authored,
    locale: &'a [u8; 128],
    // Canonical identities model exact interned contents; array slots are
    // retained separately, including duplicates. These are not Wii addresses.
    names: BTreeMap<&'a str, usize>,
}
impl<'a> Query<'a> {
    fn intern(&mut self, name: &'a str) -> usize {
        let next = self.names.len() + 1;
        *self.names.entry(name).or_insert(next)
    }
}
impl super::inventory_commands::SketchNames for Query<'_> {
    fn count(&mut self) -> Result<u32> {
        u32::try_from(self.state.sketches.len()).context("Sketch count exceeds native u32")
    }
    fn name(&mut self, index: u32) -> Result<(u32, usize)> {
        let name = self
            .state
            .sketches
            .get(index as usize)
            .context("Sketch query index outside authored storage")?;
        Ok((
            1,
            *self
                .names
                .get(c_name(name))
                .context("Sketch identity not interned")?,
        ))
    }
}
impl super::inventory_commands::AddPreview for Query<'_> {
    fn is_sketch_family(&mut self, item: &str) -> Result<bool> {
        Ok(super::native_names::compare_prefix(item.as_bytes(), b"Sketch", 6, self.locale).is_eq())
    }
    fn sketch_count(&mut self, index: u32) -> Result<i32> {
        Ok(self.state.counts.get(index as usize).copied().unwrap_or(0))
    }
    fn sketch_maximum(&mut self, index: u32) -> Result<f32> {
        Ok(self.state.maxima.get(index as usize).copied().unwrap_or(0.))
    }
    fn is_registered_powerup(&mut self, handle: usize) -> Result<bool> {
        Ok(super::inventory_commands::POWERUP_NAMES
            .iter()
            .any(|name| self.names.get(name) == Some(&handle)))
    }
    fn powerup_values(&mut self) -> Result<[f32; 4]> {
        self.state.powerup_values()
    }
    fn matches_selected(&mut self, item: &str, selected: &str) -> Result<bool> {
        Ok(super::native_names::compare(item.as_bytes(), selected.as_bytes(), self.locale).is_eq())
    }
    fn scalar_value(&mut self, item: &str) -> Result<f32> {
        self.state.floats.get(item).copied().with_context(|| {
            format!("Inventory {item} unavailable; native getter destination is uninitialized")
        })
    }
    fn scalar_maximum(&mut self, item: &str) -> Result<f32> {
        Ok(self
            .state
            .floats
            .get(&format!("{item}Max"))
            .copied()
            .unwrap_or(0.))
    }
}
