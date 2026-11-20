//! Scalar input binding reads recovered from 0x80225c00. Character-property
//! words are runtime storage, not automatically the cooked defaults: native
//! initialization/name remapping must happen before a caller supplies them.
use super::behavior::Document;
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;

#[derive(Debug, PartialEq)]
pub enum Scalar {
    Boolean(bool),
    Integer(i32),
    Real(f32),
}
pub struct Binding {
    source: u8,
    index: usize,
    kind: u8,
}
pub struct Bindings {
    members: BTreeMap<(String, String), Binding>,
}
impl Bindings {
    pub fn compile(graph: &Document) -> Result<Self> {
        let data = graph.single("hkbBehaviorGraphData")?;
        let mut members = BTreeMap::new();
        for (id, object) in &graph.objects {
            let Some(reference) = object.fields["variableBindingSet"]["ref"].as_str() else {
                continue;
            };
            let set = graph
                .objects
                .get(reference)
                .context("Missing variable binding set")?;
            ensure!(
                set.klass == "hkbVariableBindingSet",
                "Invalid variable binding set"
            );
            for b in set.fields["bindings"]
                .as_array()
                .context("Missing bindings")?
            {
                let member = b["memberPath"]
                    .as_str()
                    .context("Missing bound member path")?;
                ensure!(
                    b["bitIndex"] == -1,
                    "Bit-field bindings are not implemented"
                );
                let source = b["bindingType"]
                    .as_u64()
                    .context("Invalid binding source")?;
                ensure!(source <= 1, "Unknown binding source");
                let index = b["variableIndex"]
                    .as_u64()
                    .context("Invalid binding variable index")?
                    as usize;
                let infos = data[if source == 0 {
                    "variableInfos"
                } else {
                    "characterPropertyInfos"
                }]
                .as_array()
                .context("Missing binding type table")?;
                let kind = infos
                    .get(index)
                    .context("Binding index exceeds type table")?["type"]
                    .as_u64()
                    .context("Invalid bound variable type")?;
                ensure!(
                    kind <= 4 || (6..=8).contains(&kind),
                    "Unsupported variable binding type {kind}"
                );
                ensure!(
                    object.fields.get(member).is_some(),
                    "Nested/missing bound member {id}/{member}"
                );
                ensure!(
                    members
                        .insert(
                            (id.clone(), member.into()),
                            Binding {
                                source: source as u8,
                                index,
                                kind: kind as u8
                            }
                        )
                        .is_none(),
                    "Duplicate member binding"
                );
            }
        }
        Ok(Self { members })
    }
    /// Havok vector/quaternion words index the character-owned quad array.
    /// Character-property quads need their own runtime store and fail explicitly.
    pub fn read_quad(
        &self,
        object: &str,
        member: &str,
        variables: &[u32],
        quads: &[[f32; 4]],
    ) -> Result<Option<[f32; 4]>> {
        let Some(binding) = self.members.get(&(object.into(), member.into())) else {
            return Ok(None);
        };
        ensure!(
            (6..=8).contains(&binding.kind),
            "Bound member {object}/{member} is not a quad"
        );
        ensure!(
            binding.source == 0,
            "Character property quad runtime values are unavailable"
        );
        let index = *variables
            .get(binding.index)
            .context("Missing quad index word")? as usize;
        let value = *quads
            .get(index)
            .context("Quad binding index exceeds backing storage")?;
        ensure!(value.iter().all(|v| v.is_finite()), "Non-finite bound quad");
        Ok(Some(value))
    }
    pub fn len(&self) -> usize {
        self.members.len()
    }
    /// Read an input binding. Output-role binding selection/writeback is a
    /// separate operation and must not call this as an unconditional copy.
    pub fn read(
        &self,
        object: &str,
        member: &str,
        variables: &[u32],
        properties: Option<&[u32]>,
    ) -> Result<Option<Scalar>> {
        let Some(b) = self.members.get(&(object.into(), member.into())) else {
            return Ok(None);
        };
        let words = if b.source == 0 {
            variables
        } else {
            properties.context("Character property runtime values are unavailable")?
        };
        let word = *words.get(b.index).context("Missing bound scalar word")?;
        Ok(Some(match b.kind {
            0 => Scalar::Boolean(word != 0),
            // Wii reads the first byte/halfword of each four-byte value slot.
            1 => Scalar::Integer((word >> 24) as u8 as i8 as i32),
            2 => Scalar::Integer((word >> 16) as u16 as i16 as i32),
            3 => Scalar::Integer(word as i32),
            4 => Scalar::Real(f32::from_bits(word)),
            _ => anyhow::bail!("Quad binding {object}/{member} requires a quad read"),
        }))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn narrow_wii_values_read_high_bytes_and_missing_properties_fail() -> Result<()> {
        let mut members = BTreeMap::new();
        for (name, kind) in [("byte", 1), ("short", 2), ("word", 3)] {
            members.insert(
                ("node".into(), name.into()),
                Binding {
                    source: 1,
                    index: 0,
                    kind,
                },
            );
        }
        let bindings = Bindings { members };
        assert!(bindings.read("node", "short", &[], None).is_err());
        let words = [0xfffe1234];
        assert_eq!(
            bindings.read("node", "byte", &[], Some(&words))?,
            Some(Scalar::Integer(-1))
        );
        assert_eq!(
            bindings.read("node", "short", &[], Some(&words))?,
            Some(Scalar::Integer(-2))
        );
        assert_eq!(
            bindings.read("node", "word", &[], Some(&words))?,
            Some(Scalar::Integer(0xfffe1234u32 as i32))
        );
        Ok(())
    }
    #[test]
    #[ignore = "requires original locally extracted Mickey graph"]
    fn compiles_every_mickey_binding_and_live_blend_parameter() -> Result<()> {
        let graph = Document::load(&std::fs::read(
            "target/managed-player-assets/MickeyBehavior.hkx_wii",
        )?)?;
        let bindings = Bindings::compile(&graph)?;
        assert_eq!(bindings.len(), 47);
        let mut state = super::super::behavior_state::State::new(&graph)?;
        state.set_real(13, 0.75)?;
        assert_eq!(state.real_member(&graph, "2:2576", "blendParameter")?, 0.75);
        Ok(())
    }
}

#[cfg(test)]
mod quad_compatibility_tests {
    use super::*;
    #[test]
    fn indexed_quad_preserves_all_lanes_and_rejects_scalar_access() -> Result<()> {
        let bindings = Bindings {
            members: BTreeMap::from([(
                ("node".into(), "targetWS".into()),
                Binding {
                    source: 0,
                    index: 0,
                    kind: 7,
                },
            )]),
        };
        let values = [[9.; 4], [1., 2., 3., 4.]];
        assert_eq!(
            bindings.read_quad("node", "targetWS", &[1], &values)?,
            Some(values[1])
        );
        assert!(bindings
            .read_quad("node", "targetWS", &[2], &values)
            .is_err());
        assert!(bindings.read("node", "targetWS", &[1], None).is_err());
        Ok(())
    }
}
