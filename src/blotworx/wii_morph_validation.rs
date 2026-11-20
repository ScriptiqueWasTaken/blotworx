//! Original DBC morph geometry must agree with its authored INDEX stream.
#[cfg(debug_assertions)]
pub(super) fn validate() -> anyhow::Result<()> {
    use anyhow::{Context, ensure};
    use epic_mickey_lib_rs::{nif_file::*, packfile::Packfile};
    use std::collections::BTreeMap;
    let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
    let pak = Packfile::from_binary_path(
        std::path::Path::new(&root)
            .join("packfiles/dbc_1st_zonea.pak")
            .to_string_lossy()
            .into_owned(),
    );
    let file = pak
        .files
        .iter()
        .find(|f| f.path.ends_with("DBC_ZoneA_Machine_Toaster.nif_wii"))
        .context("Original morph machine missing")?;
    let nif = NifFile::load_bytes(&file.data).map_err(anyhow::Error::msg)?;
    let mesh =
        nif.blocks
            .iter()
            .find_map(|b| match b {
                NiObject::NiMesh(m)
                    if m.modifiers.iter().flatten().any(|id| {
                        nif.blocks[*id as usize].type_name() == "NiMorphMeshModifier"
                    }) =>
                {
                    Some(m)
                }
                _ => None,
            })
            .context("Original morph mesh missing")?;
    let decoded = crate::wii_mesh::extract(&nif, mesh)?;
    ensure!(
        decoded.mesh.colors.is_some() && decoded.mesh.uvs.is_some(),
        "Morph lost independent colour/UV attributes"
    );
    ensure!(
        decoded.source_indices.len() == decoded.mesh.positions.len(),
        "Morph POSITION source mapping missing"
    );
    let mut actual = BTreeMap::new();
    let mut expected = BTreeMap::new();
    let canonical = |a: [u32; 3]| {
        let i = if a[0] <= a[1] && a[0] <= a[2] {
            0
        } else if a[1] <= a[2] {
            1
        } else {
            2
        };
        [a[i], a[(i + 1) % 3], a[(i + 2) % 3]]
    };
    for tri in decoded.mesh.indices.chunks_exact(3) {
        let source = std::array::from_fn(|i| decoded.source_indices[tri[i] as usize]);
        *actual.entry(canonical(source)).or_insert(0usize) += 1;
    }
    let index_ref = mesh
        .stream_refs
        .iter()
        .find(|r| r.semantics.iter().any(|s| s.name == "INDEX"))
        .context("Authored INDEX missing")?;
    let NiObject::NiDataStream(stream) = &nif.blocks[index_ref.stream_link as usize] else {
        anyhow::bail!("INDEX not a stream")
    };
    let region = &stream.regions[index_ref.submesh_region_indices[0] as usize];
    let stride = vertex_stride(stream);
    ensure!(stride == 2, "Original index format changed");
    let mut indices = Vec::new();
    for i in region.start_index..region.start_index + region.count {
        let o = i as usize * 2;
        indices.push(u16::from_be_bytes(stream.buffer[o..o + 2].try_into()?) as u32);
    }
    for tri in indices.chunks_exact(3) {
        if tri[0] != tri[1] && tri[1] != tri[2] && tri[0] != tri[2] {
            *expected.entry(canonical(tri.try_into()?)).or_insert(0usize) += 1;
        }
    }
    ensure!(
        actual == expected,
        "GX morph faces/winding differ from authored INDEX topology"
    );
    let (legacy, sources) =
        extract_mesh_with_source_indices(&nif, mesh).context("Shared library fallback failed")?;
    let mut fallback = BTreeMap::new();
    for tri in legacy.indices.chunks_exact(3) {
        *fallback
            .entry(canonical(std::array::from_fn(|i| sources[tri[i] as usize])))
            .or_insert(0usize) += 1;
    }
    ensure!(
        fallback == expected,
        "Shared library still misreads morph display list"
    );
    println!(
        "PASS: original DBC morph display list matches authored INDEX faces and winding ({} triangles); independent colours/UVs and POSITION mapping retained in both decoders",
        decoded.mesh.indices.len() / 3
    );
    Ok(())
}
