//! Original system traversal DOL802dd8ac/802ddac0. World operations, locks,
//! retained handles and notifications belong to the required backend.
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::hkx_file::HkxFile;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Body,
    Phantom,
    Action,
    Constraint,
}
pub struct System {
    pub bodies: Vec<Option<usize>>,
    pub phantoms: Vec<Option<usize>>,
    pub actions: Vec<Option<usize>>,
    pub constraints: Vec<Option<usize>>,
    pub active: i8,
}
/// Serialized system records. Handles are validated data-section offsets,
/// never host addresses; array order and null holes remain intact.
pub struct Imported {
    pub offset: usize,
    pub system: System,
}
fn pc_2012(file: &HkxFile) -> bool {
    file.header.pointer_size == 4
        && file.header.endianness == 1
        && file.header.padding_option == 0
        && file.header.base_format == 1
        && file.header.contents_version == "hk_2012.1.0-r1"
}
/// REL382730..778: root findObjectByType(hkpPhysicsData,null), then
/// its first system. DOL801d37ec uses variant order and runtime class first,
/// falling back to the declared class name only when no runtime class exists.
pub fn select_first(file: &HkxFile, systems: &[Imported]) -> Result<Option<usize>> {
    let little = pc_2012(file);
    ensure!(
        file.header.pointer_size == 4 && (file.header.endianness == 0 || little),
        "Unsupported system-selection ABI"
    );
    let section = usize::try_from(file.header.contents_section_index)?;
    ensure!(
        file.sections
            .get(section)
            .is_some_and(|section| section.section_tag == "__data__"),
        "Cross-section resource root requires scoped system handles"
    );
    let data = file
        .section_data
        .get(section)
        .context("Missing root data")?;
    let root = usize::try_from(file.header.contents_section_offset)?;
    let word = |at: usize| -> Result<u32> {
        let bytes = data
            .get(at..at.checked_add(4).context("Root offset overflow")?)
            .context("Truncated root word")?
            .try_into()?;
        Ok(if little {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        })
    };
    let pointer = |at: usize| -> Result<Option<usize>> {
        if let Some(f) = file.local_fixups[section]
            .iter()
            .find(|f| f.src_offset as usize == at)
        {
            return Ok(Some(f.dst_offset as usize));
        }
        if let Some(f) = file.global_fixups[section]
            .iter()
            .find(|f| f.src_offset as usize == at)
        {
            ensure!(
                f.dst_section as usize == section,
                "Cross-section root selection unsupported"
            );
            return Ok(Some(f.dst_offset as usize));
        }
        ensure!(word(at)? == 0, "Unresolved root pointer");
        Ok(None)
    };
    let class = |at: usize| -> Result<Option<&str>> {
        let Some(f) = file.virtual_fixups[section]
            .iter()
            .find(|f| f.src_offset as usize == at)
        else {
            return Ok(None);
        };
        let bytes = file
            .section_data
            .get(f.section_index as usize)
            .and_then(|d| d.get(f.class_name_offset as usize..))
            .context("Invalid root class")?;
        let end = bytes
            .iter()
            .position(|b| *b == 0)
            .context("Unterminated root class")?;
        Ok(Some(std::str::from_utf8(&bytes[..end])?))
    };
    ensure!(
        class(root)? == Some("hkRootLevelContainer"),
        "Physics resource root is not a named-variant container"
    );
    let count = word(root + 4)? as i32;
    ensure!(count >= 0, "Negative root variant count");
    if count == 0 {
        return Ok(None);
    }
    let variants = pointer(root)?.context("Missing root variants")?;
    ensure!(
        variants
            .checked_add(
                (count as usize)
                    .checked_mul(12)
                    .context("Variant size overflow")?
            )
            .context("Variant end overflow")?
            <= data.len(),
        "Truncated root variants"
    );
    for index in 0..count as usize {
        let at = variants + index * 12;
        let object = pointer(at + 8)?;
        let runtime = object.map(class).transpose()?.flatten();
        let declared = if runtime.is_none() {
            pointer(at + 4)?
                .map(|name| -> Result<&str> {
                    let bytes = data.get(name..).context("Invalid declared class pointer")?;
                    let end = bytes
                        .iter()
                        .position(|b| *b == 0)
                        .context("Unterminated declared class")?;
                    Ok(std::str::from_utf8(&bytes[..end])?)
                })
                .transpose()?
        } else {
            None
        };
        if runtime.or(declared) != Some("hkpPhysicsData") {
            continue;
        }
        let Some(physics) = object else {
            return Ok(None);
        };
        let count = word(physics + 0x10)? as i32;
        ensure!(count >= 0, "Negative physics-data system count");
        if count == 0 {
            return Ok(None);
        }
        let array = pointer(physics + 0xc)?.context("Missing physics-data systems")?;
        let selected = pointer(array)?.context("Null first physics system")?;
        ensure!(
            systems.iter().any(|system| system.offset == selected),
            "Selected physics system has no imported record"
        );
        return Ok(Some(selected));
    }
    Ok(None)
}
pub fn import(file: &HkxFile) -> Result<Vec<Imported>> {
    let little = pc_2012(file);
    ensure!(
        file.header.pointer_size == 4
            && ((file.header.endianness == 0
                && file.header.contents_version.starts_with("Havok-7.0"))
                || little),
        "Unsupported physics-system ABI"
    );
    let section = file
        .sections
        .iter()
        .position(|s| s.section_tag == "__data__")
        .context("Missing system data section")?;
    let data = &file.section_data[section];
    let word = |at: usize| -> Result<u32> {
        let bytes = data
            .get(at..at.checked_add(4).context("System offset overflow")?)
            .context("Truncated system word")?
            .try_into()?;
        Ok(if little {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        })
    };
    let pointer = |at: usize| -> Result<Option<usize>> {
        let local = file.local_fixups[section]
            .iter()
            .find(|f| f.src_offset as usize == at)
            .map(|f| f.dst_offset as usize);
        let global = file.global_fixups[section]
            .iter()
            .find(|f| f.src_offset as usize == at);
        ensure!(
            local.is_none() || global.is_none(),
            "Ambiguous system pointer"
        );
        let target = if let Some(target) = local {
            Some(target)
        } else if let Some(f) = global {
            ensure!(
                f.dst_section as usize == section,
                "Cross-section system member unsupported"
            );
            Some(f.dst_offset as usize)
        } else {
            ensure!(word(at)? == 0, "Unresolved system pointer {at:#x}");
            None
        };
        if let Some(target) = target {
            ensure!(target < data.len(), "System pointer out of range");
        }
        Ok(target)
    };
    let array = |at: usize| -> Result<Vec<Option<usize>>> {
        let count = word(at + 4)? as i32;
        ensure!(count >= 0, "Negative system array count");
        if count == 0 {
            return Ok(vec![]);
        }
        let base = pointer(at)?.context("Nonempty system array has no pointer")?;
        let end = base
            .checked_add(
                (count as usize)
                    .checked_mul(4)
                    .context("System array size overflow")?,
            )
            .context("System array end overflow")?;
        ensure!(end <= data.len(), "Truncated system array");
        (0..count as usize).map(|i| pointer(base + i * 4)).collect()
    };
    let mut result = Vec::new();
    for fix in &file.virtual_fixups[section] {
        let name = file
            .section_data
            .get(fix.section_index as usize)
            .and_then(|d| d.get(fix.class_name_offset as usize..))
            .context("Invalid system class fixup")?
            .split(|b| *b == 0)
            .next()
            .unwrap();
        if name != b"hkpPhysicsSystem" {
            continue;
        }
        let at = fix.src_offset as usize;
        result.push(Imported {
            offset: at,
            system: System {
                bodies: array(at + 8)?,
                constraints: array(at + 0x14)?,
                actions: array(at + 0x20)?,
                phantoms: array(at + 0x2c)?,
                active: *data.get(at + 0x40).context("Missing system activity")? as i8,
            },
        });
    }
    Ok(result)
}
pub trait Backend {
    fn bulk(
        &mut self,
        kind: Kind,
        add: bool,
        handles: &[Option<usize>],
        active: bool,
    ) -> Result<()>;
    fn single(&mut self, kind: Kind, add: bool, handle: usize, active: bool) -> Result<()>;
    fn constraint_has_endpoint(&mut self, handle: usize) -> Result<bool>;
}
fn objects(
    backend: &mut impl Backend,
    kind: Kind,
    add: bool,
    handles: &[Option<usize>],
    active: bool,
) -> Result<()> {
    // Add skips empty arrays; removal still calls the zero-length bulk path.
    if add && handles.is_empty() {
        return Ok(());
    };
    if handles.iter().all(Option::is_some) {
        backend.bulk(kind, add, handles, active)?;
    } else {
        for &handle in handles {
            if let Some(handle) = handle {
                backend.single(kind, add, handle, active)?;
            }
        }
    }
    Ok(())
}
impl System {
    pub fn add(&self, backend: &mut impl Backend) -> Result<()> {
        objects(backend, Kind::Body, true, &self.bodies, self.active != 0)?;
        objects(backend, Kind::Phantom, true, &self.phantoms, false)?;
        for &handle in &self.actions {
            if let Some(h) = handle {
                backend.single(Kind::Action, true, h, false)?;
            }
        }
        for &handle in &self.constraints {
            if let Some(h) = handle {
                if backend.constraint_has_endpoint(h)? {
                    backend.single(Kind::Constraint, true, h, false)?;
                }
            }
        }
        Ok(())
    }
    pub fn remove(&self, backend: &mut impl Backend) -> Result<()> {
        for &handle in &self.constraints {
            if let Some(h) = handle {
                backend.single(Kind::Constraint, false, h, false)?;
            }
        }
        // Backend must check the live world lock per action, not once here.
        for &handle in &self.actions {
            if let Some(h) = handle {
                backend.single(Kind::Action, false, h, false)?;
            }
        }
        objects(backend, Kind::Body, false, &self.bodies, false)?;
        objects(backend, Kind::Phantom, false, &self.phantoms, false)?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Trace(Vec<(Kind, bool, Vec<usize>, bool)>);
    impl Backend for Trace {
        fn bulk(&mut self, k: Kind, a: bool, h: &[Option<usize>], v: bool) -> Result<()> {
            self.0
                .push((k, a, h.iter().map(|h| h.unwrap()).collect(), v));
            Ok(())
        }
        fn single(&mut self, k: Kind, a: bool, h: usize, v: bool) -> Result<()> {
            self.0.push((k, a, vec![h], v));
            Ok(())
        }
        fn constraint_has_endpoint(&mut self, h: usize) -> Result<bool> {
            Ok(h != 8)
        }
    }
    #[test]
    fn system_preserves_sparse_branch_order_and_constraint_admission() -> Result<()> {
        let system = System {
            bodies: vec![Some(1), Some(2)],
            phantoms: vec![Some(3), None, Some(4)],
            actions: vec![Some(5), None],
            constraints: vec![Some(8), Some(9)],
            active: -1,
        };
        let mut trace = Trace::default();
        system.add(&mut trace)?;
        assert_eq!(
            trace.0,
            vec![
                (Kind::Body, true, vec![1, 2], true),
                (Kind::Phantom, true, vec![3], false),
                (Kind::Phantom, true, vec![4], false),
                (Kind::Action, true, vec![5], false),
                (Kind::Constraint, true, vec![9], false)
            ]
        );
        trace.0.clear();
        system.remove(&mut trace)?;
        assert_eq!(
            trace.0.iter().map(|e| e.0).collect::<Vec<_>>(),
            [
                Kind::Constraint,
                Kind::Constraint,
                Kind::Action,
                Kind::Body,
                Kind::Phantom,
                Kind::Phantom
            ]
        );
        assert_eq!(trace.0[0].2, [8]);
        Ok(())
    }
    #[test]
    fn empty_system_add_skips_but_removal_uses_both_bulk_paths() -> Result<()> {
        let system = System {
            bodies: vec![],
            phantoms: vec![],
            actions: vec![],
            constraints: vec![],
            active: 0,
        };
        let mut trace = Trace::default();
        system.add(&mut trace)?;
        assert!(trace.0.is_empty());
        system.remove(&mut trace)?;
        assert_eq!(
            trace.0,
            vec![
                (Kind::Body, false, vec![], false),
                (Kind::Phantom, false, vec![], false)
            ]
        );
        Ok(())
    }
}
#[cfg(test)]
mod source_selection_tests {
    use super::*;
    use epic_mickey_lib_rs::hkx_file::{
        HkxGlobalFixup, HkxHeader, HkxLocalFixup, HkxSectionHeader, HkxVirtualFixup,
    };
    fn fixture(little: bool) -> HkxFile {
        let mut data = vec![0; 0x600];
        let word = |data: &mut [u8], at: usize, value: u32| {
            data[at..at + 4].copy_from_slice(&if little {
                value.to_le_bytes()
            } else {
                value.to_be_bytes()
            })
        };
        for (at, value) in [
            (4, 1),
            (0x60, 2),
            (0x10c, 1),
            (0x18c, 3),
            (0x198, 2),
            (0x1a4, 1),
            (0x1b0, 2),
        ] {
            word(&mut data, at, value);
        }
        data[0x1c0] = 1;
        let mut names = vec![];
        let mut virtuals = vec![];
        for (at, name) in [
            (0, "hkRootLevelContainer"),
            (0x50, "hkpPhysicsData"),
            (0x100, "hkpPhysicsSystem"),
            (0x180, "hkpPhysicsSystem"),
        ] {
            let offset = names.len() as u32;
            names.extend_from_slice(name.as_bytes());
            names.push(0);
            virtuals.push(HkxVirtualFixup {
                src_offset: at,
                section_index: 1,
                class_name_offset: offset,
            });
        }
        HkxFile {
            header: HkxHeader {
                pointer_size: 4,
                contents_section_index: 0,
                contents_section_offset: 0,
                endianness: u8::from(little),
                padding_option: 0,
                base_format: 1,
                contents_version: if little {
                    "hk_2012.1.0-r1"
                } else {
                    "Havok-7.0.0-r1"
                }
                .into(),
                ..Default::default()
            },
            sections: vec![HkxSectionHeader {
                section_tag: "__data__".into(),
                ..Default::default()
            }],
            section_data: vec![data, names],
            local_fixups: vec![([
                (0, 0x20),
                (0x5c, 0x80),
                (0x108, 0x240),
                (0x188, 0x260),
                (0x194, 0x280),
                (0x1a0, 0x2a0),
                (0x1ac, 0x2b0),
            ])
            .into_iter()
            .map(|(src, dst)| HkxLocalFixup {
                src_offset: src,
                dst_offset: dst,
            })
            .collect()],
            global_fixups: vec![([
                (0x28, 0x50),
                (0x80, 0x180),
                (0x84, 0x100),
                (0x240, 0x300),
                (0x260, 0x380),
                (0x268, 0x300),
                (0x280, 0x450),
                (0x2a0, 0x480),
                (0x2b0, 0x4c0),
                (0x2b4, 0x4e0),
            ])
            .into_iter()
            .map(|(src, dst)| HkxGlobalFixup {
                src_offset: src,
                dst_section: 0,
                dst_offset: dst,
            })
            .collect()],
            virtual_fixups: vec![virtuals],
            ..Default::default()
        }
    }
    #[test]
    fn both_source_abis_preserve_selected_system_slots_and_auxiliary_order() -> Result<()> {
        for little in [false, true] {
            let file = fixture(little);
            let imported = import(&file)?;
            assert_eq!(
                imported.iter().map(|s| s.offset).collect::<Vec<_>>(),
                [0x100, 0x180]
            );
            assert_eq!(select_first(&file, &imported)?, Some(0x180));
            let selected = &imported[1].system;
            assert_eq!(selected.bodies, [Some(0x380), None, Some(0x300)]);
            assert_eq!(selected.constraints, [Some(0x450), None]);
            assert_eq!(selected.actions, [Some(0x480)]);
            assert_eq!(selected.phantoms, [Some(0x4c0), Some(0x4e0)]);
            assert_eq!(selected.active, 1);
        }
        Ok(())
    }
    #[test]
    fn malformed_selected_arrays_and_wrong_pc_abi_fail() -> Result<()> {
        let mut file = fixture(true);
        file.section_data[0][0x18c..0x190].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(import(&file).is_err());
        let mut file = fixture(true);
        file.header.padding_option = 1;
        assert!(import(&file).is_err());
        let mut file = fixture(true);
        file.global_fixups[0].retain(|f| f.src_offset != 0x80);
        let imported = import(&file)?;
        assert!(select_first(&file, &imported).is_err());
        Ok(())
    }
}
