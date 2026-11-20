//! Resolve original HKX shape identities without flattening shared children.
use super::shape_bounds::{
    BoxShape, ConvexBounds, ExtendedMeshBounds, ListBounds, Shape, ShapeGraph,
};
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::hkx_file::HkxFile;
use std::collections::{HashMap, HashSet};
pub type Address = (usize, usize);

/// Roots are section/offset shape addresses, in caller-owned body order.
/// Unsupported classes fail explicitly; geometry decoding is a separate step.
pub fn import(file: &HkxFile, roots: &[Address]) -> Result<(ShapeGraph, Vec<usize>)> {
    ensure!(
        file.header.pointer_size == 4
            && file.header.endianness == 0
            && file.header.contents_version.starts_with("Havok-7.0"),
        "Unsupported bounds HKX layout"
    );
    let mut pointers = HashMap::new();
    let mut classes = HashMap::new();
    for (section, fixups) in file.local_fixups.iter().enumerate() {
        for f in fixups {
            ensure!(
                pointers
                    .insert(
                        (section, f.src_offset as usize),
                        (section, f.dst_offset as usize)
                    )
                    .is_none(),
                "Duplicate shape pointer fixup"
            );
        }
    }
    for (section, fixups) in file.global_fixups.iter().enumerate() {
        for f in fixups {
            ensure!(
                pointers
                    .insert(
                        (section, f.src_offset as usize),
                        (f.dst_section as usize, f.dst_offset as usize)
                    )
                    .is_none(),
                "Duplicate shape pointer fixup"
            );
        }
    }
    for (section, fixups) in file.virtual_fixups.iter().enumerate() {
        for f in fixups {
            let bytes = file
                .section_data
                .get(f.section_index as usize)
                .and_then(|d| d.get(f.class_name_offset as usize..))
                .context("Missing shape class name")?;
            let end = bytes
                .iter()
                .position(|&v| v == 0)
                .context("Unterminated shape class name")?;
            let name = std::str::from_utf8(&bytes[..end]).context("Invalid shape class name")?;
            ensure!(
                classes
                    .insert((section, f.src_offset as usize), name)
                    .is_none(),
                "Duplicate shape class fixup"
            );
        }
    }
    let pointer = |address: Address| {
        pointers
            .get(&address)
            .copied()
            .with_context(|| format!("Missing shape pointer {address:?}"))
    };
    enum Pending {
        List(ListBounds),
        Mopp,
        Translated([f32; 4]),
        Transformed(super::native_hk_transform::Transform),
    }
    enum Work {
        Visit(Address),
        Finish(Address, Pending, Vec<Address>),
    }
    let mut graph = ShapeGraph::default();
    let mut resolved = HashMap::new();
    let mut active = HashSet::new();
    let mut stack: Vec<_> = roots.iter().rev().copied().map(Work::Visit).collect();
    while let Some(work) = stack.pop() {
        match work {
            Work::Visit(address) => {
                if resolved.contains_key(&address) {
                    continue;
                }
                ensure!(active.insert(address), "Cyclic shape graph at {address:?}");
                let (section, offset) = address;
                let data = file
                    .section_data
                    .get(section)
                    .context("Missing shape section")?;
                let field =
                    |relative: usize| offset.checked_add(relative).context("Shape field overflow");
                let name = *classes
                    .get(&address)
                    .with_context(|| format!("Missing shape class at {address:?}"))?;
                let (pending, children) = match name {
                    "hkpBoxShape" | "hkpCylinderShape" | "hkpConvexVerticesShape" => {
                        let shape = if name == "hkpBoxShape" {
                            Shape::Box(BoxShape::decode(data, offset)?)
                        } else if name == "hkpCylinderShape" {
                            Shape::Cylinder(super::shape_bounds::CylinderShape::decode(
                                data, offset,
                            )?)
                        } else {
                            Shape::Convex(ConvexBounds::decode(data, offset)?)
                        };
                        resolved.insert(address, graph.insert(shape)?);
                        active.remove(&address);
                        continue;
                    }
                    "hkpMoppBvTreeShape" => {
                        (Pending::Mopp, vec![pointer((section, field(0x34)?))?])
                    }
                    "hkpConvexTranslateShape" => {
                        // Wii reflection/retail door asset: child+18,
                        // hkVector4 translation+20; no flattening of identities.
                        let start = field(0x20)?;
                        let bytes = data
                            .get(start..start + 16)
                            .context("Truncated convex translation")?;
                        let translation = std::array::from_fn(|i| {
                            f32::from_be_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
                        });
                        (
                            Pending::Translated(translation),
                            vec![pointer((section, field(0x18)?))?],
                        )
                    }
                    "hkpConvexTransformShape" => {
                        let transform =
                            super::native_hk_transform::decode_shape_transform(data, offset)?;
                        (
                            Pending::Transformed(transform),
                            vec![pointer((section, field(0x18)?))?],
                        )
                    }
                    "hkpExtendedMeshShape" | "hkpStorageExtendedMeshShape" => {
                        resolved.insert(
                            address,
                            graph.insert(Shape::ExtendedMesh(ExtendedMeshBounds::decode(
                                data, offset,
                            )?))?,
                        );
                        active.remove(&address);
                        continue;
                    }
                    "hkpListShape" => {
                        let bounds = ListBounds::decode(data, offset)?;
                        let at = field(0x1c)?;
                        let count = u32::from_be_bytes(
                            data.get(at..at + 4)
                                .context("Truncated list count")?
                                .try_into()?,
                        ) as usize;
                        let mut children = Vec::new();
                        if count != 0 {
                            let (child_section, start) = pointer((section, field(0x18)?))?;
                            let size = count.checked_mul(16).context("List child size overflow")?;
                            let end = start
                                .checked_add(size)
                                .context("List child offset overflow")?;
                            ensure!(
                                file.section_data
                                    .get(child_section)
                                    .and_then(|d| d.get(start..end))
                                    .is_some(),
                                "Truncated list child array"
                            );
                            children.reserve(count);
                            for i in 0..count {
                                children.push(pointer((child_section, start + i * 16))?);
                            }
                        }
                        (Pending::List(bounds), children)
                    }
                    _ => anyhow::bail!("Unsupported bounds shape {name} at {address:?}"),
                };
                let visits: Vec<_> = children.iter().rev().copied().map(Work::Visit).collect();
                stack.push(Work::Finish(address, pending, children));
                stack.extend(visits);
            }
            Work::Finish(address, pending, children) => {
                let ids: Vec<usize> = children
                    .iter()
                    .map(|a| resolved.get(a).copied().context("Unresolved shape child"))
                    .collect::<Result<_>>()?;
                let shape = match pending {
                    Pending::List(bounds) => Shape::List {
                        bounds,
                        children: ids.into(),
                    },
                    Pending::Mopp => Shape::Mopp { child: ids[0] },
                    Pending::Translated(translation) => Shape::Translated {
                        child: ids[0],
                        translation,
                    },
                    Pending::Transformed(transform) => Shape::Transformed {
                        child: ids[0],
                        transform,
                    },
                };
                resolved.insert(address, graph.insert(shape)?);
                active.remove(&address);
            }
        }
    }
    let ids = roots
        .iter()
        .map(|a| resolved.get(a).copied().context("Unresolved bounds root"))
        .collect::<Result<_>>()?;
    Ok((graph, ids))
}

#[cfg(test)]
mod tests {
    use super::*;
    use epic_mickey_lib_rs::hkx_file::{HkxGlobalFixup, HkxHeader, HkxVirtualFixup};
    fn fixture() -> HkxFile {
        let mut data = vec![0; 0xa0];
        for (at, value) in [(0x10, 0.25f32), (0x20, 1.), (0x24, 2.), (0x28, 3.)] {
            data[at..at + 4].copy_from_slice(&value.to_be_bytes());
        }
        HkxFile {
            header: HkxHeader {
                pointer_size: 4,
                endianness: 0,
                contents_version: "Havok-7.0.0-r1".into(),
                ..Default::default()
            },
            section_data: vec![data, b"hkpBoxShape\0hkpMoppBvTreeShape\0".to_vec()],
            global_fixups: vec![
                vec![HkxGlobalFixup {
                    src_offset: 0x74,
                    dst_section: 0,
                    dst_offset: 0,
                }],
                vec![],
            ],
            virtual_fixups: vec![
                vec![
                    HkxVirtualFixup {
                        src_offset: 0,
                        section_index: 1,
                        class_name_offset: 0,
                    },
                    HkxVirtualFixup {
                        src_offset: 0x40,
                        section_index: 1,
                        class_name_offset: 12,
                    },
                ],
                vec![],
            ],
            ..Default::default()
        }
    }
    #[test]
    fn fixups_preserve_root_identity_and_reject_cycles_or_missing_children() {
        let mut file = fixture();
        let (mut graph, roots) = import(&file, &[(0, 0x40), (0, 0), (0, 0x40)]).unwrap();
        assert_eq!(roots[0], roots[2]);
        assert_ne!(roots[0], roots[1]);
        let identity = super::super::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [0.; 3],
        );
        assert_eq!(
            graph.aabb(roots[0], &identity, 0.).unwrap(),
            [[-1.25, -2.25, -3.25, -0.25], [1.25, 2.25, 3.25, 0.25]]
        );
        file.global_fixups[0][0].dst_offset = 0x40;
        assert!(import(&file, &[(0, 0x40)])
            .err()
            .unwrap()
            .to_string()
            .contains("Cyclic"));
        file.global_fixups[0].clear();
        assert!(import(&file, &[(0, 0x40)]).is_err());
    }
    #[test]
    fn transformed_child_bounds_compose_rotation_and_translation_and_reject_bad_wrappers() {
        let mut file = fixture();
        let class_offset = file.section_data[1].len() as u32;
        file.section_data[1].extend_from_slice(b"hkpConvexTransformShape\0");
        file.virtual_fixups[0][1].class_name_offset = class_offset;
        file.global_fixups[0][0].src_offset = 0x58;
        let transform = [
            0f32, 1., 0., 0., -1., 0., 0., 0., 0., 0., 1., 0., 10., 20., 30., 0.,
        ];
        for (i, value) in transform.iter().enumerate() {
            file.section_data[0][0x60 + i * 4..0x64 + i * 4].copy_from_slice(&value.to_be_bytes());
        }
        let (mut graph, roots) = import(&file, &[(0, 0x40), (0, 0)]).unwrap();
        let parent = super::super::native_hk_transform::from_entity(
            [0., -1., 0., 1., 0., 0., 0., 0., 1.],
            [-20., 0., 0.],
        );
        assert_eq!(
            graph.aabb(roots[0], &parent, 0.).unwrap(),
            [[-41.25, 7.75, 26.75, -0.25], [-38.75, 12.25, 33.25, 0.25]]
        );
        // The wrapper shares a child; it must not mutate the child's bounds.
        let identity = super::super::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [0.; 3],
        );
        assert_eq!(
            graph.aabb(roots[1], &identity, 0.).unwrap(),
            [[-1.25, -2.25, -3.25, -0.25], [1.25, 2.25, 3.25, 0.25]]
        );
        file.global_fixups[0][0].dst_offset = 0x40;
        assert!(import(&file, &[(0, 0x40)])
            .err()
            .unwrap()
            .to_string()
            .contains("Cyclic"));
        file.global_fixups[0][0].dst_offset = 0;
        file.section_data[0][0x60..0x64].copy_from_slice(&f32::NAN.to_be_bytes());
        assert!(import(&file, &[(0, 0x40)]).is_err());
        file.section_data[0].truncate(0x9f);
        assert!(import(&file, &[(0, 0x40)]).is_err());
    }
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn production_wheel_platform_shape_bounds() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let pak = epic_mickey_lib_rs::packfile::Packfile::from_binary(std::fs::read(
            std::path::Path::new(&root).join("packfiles/gremlinvillage_zonea.pak"),
        )?);
        for (name, count) in [
            ("gv_zonea_wheel01_dynamic_hull.hkx", 10),
            ("gv_zonea_platform01_dynamic_mesh.hkx", 1),
        ] {
            let asset = pak
                .files
                .iter()
                .find(|f| f.path.to_ascii_lowercase().contains(name))
                .context("Missing wheel collision")?;
            let file = HkxFile::load_bytes(&asset.data).map_err(anyhow::Error::msg)?;
            let mut roots = Vec::new();
            for (section, fixups) in file.virtual_fixups.iter().enumerate() {
                for f in fixups {
                    let name = &file.section_data[f.section_index as usize]
                        [f.class_name_offset as usize..];
                    if name.starts_with(b"hkpRigidBody\0") {
                        let pointer = file.global_fixups[section]
                            .iter()
                            .find(|p| p.src_offset == f.src_offset + 0x10)
                            .context("Missing body shape")?;
                        roots.push((pointer.dst_section as usize, pointer.dst_offset as usize));
                    }
                }
            }
            assert_eq!(roots.len(), count);
            let (mut graph, ids) = import(&file, &roots)?;
            let identity = super::super::native_hk_transform::from_entity(
                [1., 0., 0., 0., 1., 0., 0., 0., 1.],
                [0.; 3],
            );
            for id in ids {
                let first = graph.aabb(id, &identity, 0.)?;
                ensure!(
                    (0..3).all(|i| first[0][i] < first[1][i]),
                    "Degenerate wheel bounds"
                );
                assert_eq!(
                    graph.aabb(id, &identity, 0.)?,
                    first,
                    "Repeated query changed bounds"
                );
            }
        }
        Ok(())
    }
}
