//! Authored brush rigid body geometry; do not replace it with a guessed radius.
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::hkx_file::HkxFile;

#[derive(Clone, Debug, serde::Serialize)]
pub struct SphereBody {
    pub radius: f32,
    pub body_filter: u32,
    pub transform: [f32; 16],
    pub motion_type: u8,
    pub linear_velocity: [f32; 3],
    pub angular_velocity: [f32; 3],
}
pub fn load(file: &HkxFile) -> Result<SphereBody> {
    ensure!(
        file.header.pointer_size == 4
            && file.header.endianness == 0
            && file.header.contents_version.starts_with("Havok-7.0"),
        "Unsupported projectile Havok layout"
    );
    let section = file
        .sections
        .iter()
        .position(|s| s.section_tag == "__data__")
        .context("Missing projectile data section")?;
    let data = &file.section_data[section];
    let class = |offset: u32| -> Option<&str> {
        let fix = file
            .virtual_fixups
            .get(section)?
            .iter()
            .find(|f| f.src_offset == offset)?;
        let bytes = file
            .section_data
            .get(fix.section_index as usize)?
            .get(fix.class_name_offset as usize..)?
            .split(|b| *b == 0)
            .next()?;
        std::str::from_utf8(bytes).ok()
    };
    let bodies: Vec<_> = file.virtual_fixups[section]
        .iter()
        .filter(|f| class(f.src_offset) == Some("hkpRigidBody"))
        .collect();
    ensure!(
        bodies.len() == 1,
        "Brush projectile requires one rigid body"
    );
    let body = bodies[0].src_offset;
    let shape = file.global_fixups[section]
        .iter()
        .find(|f| f.src_offset == body + 0x10)
        .context("Missing projectile shape pointer")?;
    ensure!(
        shape.dst_section as usize == section && class(shape.dst_offset) == Some("hkpSphereShape"),
        "Brush projectile shape is not a supported sphere"
    );
    decode(data, body as usize, shape.dst_offset as usize)
}
fn decode(data: &[u8], body: usize, shape: usize) -> Result<SphereBody> {
    let float = |at: usize| -> Result<f32> {
        let n = f32::from_be_bytes(
            data.get(at..at + 4)
                .context("Truncated projectile body")?
                .try_into()?,
        );
        ensure!(n.is_finite(), "Non-finite projectile body value");
        Ok(n)
    };
    let radius = float(shape + 0x10)?;
    ensure!(radius > 0., "Non-positive projectile sphere radius");
    let mut transform = [0.; 16];
    for (i, v) in transform.iter_mut().enumerate() {
        *v = float(body + 0xf0 + i * 4)?;
    }
    ensure!(transform[15] == 1., "Invalid projectile body transform");
    let vector =
        |at: usize| -> Result<[f32; 3]> { Ok([float(at)?, float(at + 4)?, float(at + 8)?]) };
    let body_filter = u32::from_be_bytes(
        data.get(body + 0x2c..body + 0x30)
            .context("Truncated projectile body filter")?
            .try_into()?,
    );
    Ok(SphereBody {
        radius,
        body_filter,
        transform,
        motion_type: *data
            .get(body + 0xe8)
            .context("Missing projectile motion type")?,
        linear_velocity: vector(body + 0x1b0)?,
        angular_velocity: vector(body + 0x1c0)?,
    })
}
