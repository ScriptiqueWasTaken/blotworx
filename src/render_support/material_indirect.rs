//! R3MT Warp matrices (REL1795f0..179660) and GX indirect coordinates.
//! Sampling/filter precision and per-instance indirect-scale overrides still
//! require original GPU validation. Unsupported bindings reject explicitly.
use super::{attr, child};
use anyhow::{bail, ensure, Context, Result};
use roxmltree::Node;
use std::collections::BTreeSet;

/// GXSetIndTexMtx805b4810 packs each fmuls(*1024)/fctiwz into11 bits.
fn coefficient(value: f32) -> Result<i32> {
    let value = value * 1024.;
    ensure!(
        value.is_finite() && value >= i32::MIN as f32 && value < (i32::MAX as f64 + 1.) as f32,
        "Indirect coefficient cannot be converted by fctiwz"
    );
    let word = (value.trunc() as i32) & 0x7ff;
    Ok(if word & 0x400 != 0 {
        word - 0x800
    } else {
        word
    })
}
fn matrix(map: Node<'_, '_>) -> Result<[[i32; 3]; 2]> {
    ensure!(
        attr(map, "Method")? == "Warp",
        "Indirect matrix method is not yet reproduced"
    );
    let read = |name| -> Result<f32> {
        let v: f32 = attr(map, name)?.parse()?;
        ensure!(v.is_finite(), "Nonfinite indirect {name}");
        Ok(v)
    };
    srt_matrix(
        [read("ScaleS")?, read("ScaleT")?],
        read("Rotate")?,
        [read("TransS")?, read("TransT")?],
    )
}
pub(crate) fn srt_matrix(
    scale: [f32; 2],
    degrees: f32,
    translation: [f32; 2],
) -> Result<[[i32; 3]; 2]> {
    let radians = degrees * f32::from_bits(0x3c8efa35);
    let (sin, cos) = (radians as f64).sin_cos();
    let (sin, cos) = (sin as f32, cos as f32);
    let [s, t] = scale;
    Ok([
        [
            coefficient((s * cos) * 0.5)?,
            coefficient((s * sin) * 0.5)?,
            coefficient(translation[0])?,
        ],
        [
            coefficient(((-t) * sin) * 0.5)?,
            coefficient((t * cos) * 0.5)?,
            coefficient(translation[1])?,
        ],
    ])
}

fn wrap(expression: &str, name: &str) -> Result<String> {
    Ok(match name {
        "ItwOff" => expression.into(),
        "Itw0" => "0".into(),
        "Itw256" | "Itw128" | "Itw64" | "Itw32" | "Itw16" => {
            let size: i32 = name[3..].parse()?;
            format!("({expression}&{})", size * 128 - 1)
        }
        other => bail!("Unknown indirect coordinate wrap {other}"),
    })
}
pub(super) fn texture(
    stage: Node<'_, '_>,
    maps: Node<'_, '_>,
    slot: u8,
    coordinate: &str,
    uv: &mut BTreeSet<u8>,
    slots: &mut BTreeSet<u8>,
) -> Result<String> {
    let ind = child(stage, "IndirectStage")?;
    ensure!(
        attr(ind, "AlphaSel")? == "ItbaOff",
        "Indirect bump-alpha input is not yet reproduced"
    );
    let indirect_maps = maps
        .children()
        .filter(|n| n.has_tag_name("IndirectMap"))
        .collect::<Vec<_>>();
    let mut code=format!("{{vec3 bw_coord={coordinate};ivec2 bw_base=ivec2(round((bw_coord.xy/bw_coord.z)*vec2(textureSize(u_tex[{slot}],0))*128.0));ivec2 bw_offset=ivec2(0);\n");
    if attr(ind, "MatrixSel")? != "ItmOff" {
        let index = match attr(ind, "MatrixSel")? {
            "Itm0" => 0,
            "Itm1" => 1,
            "Itm2" => 2,
            other => bail!("Indirect matrix selector {other} is not yet reproduced"),
        };
        let m = matrix(
            *indirect_maps
                .get(index)
                .context("Indirect matrix definition missing")?,
        )?;
        let stage_index: usize = attr(ind, "IndStage")?.parse()?;
        let source = *indirect_maps
            .get(stage_index)
            .context("Indirect sample definition missing")?;
        let source_slot: u8 = attr(source, "SourceMapIndex")?.parse()?;
        ensure!(source_slot < 8, "Invalid indirect source map");
        slots.insert(source_slot);
        let source_map = maps
            .children()
            .filter(|n| n.has_tag_name("TextureMap"))
            .nth(source_slot as usize)
            .context("Indirect source coordinate generator missing")?;
        // REL179568..179588 selects source map for both GX order fields and
        // disables the S/T coordinate divider (GXSetIndTexCoordScale0,0).
        let source_coordinate = super::coordinate_expression(source_map, source_slot, uv)?;
        let shift = match attr(ind, "Format")? {
            "Itf8" => 0,
            "Itf5" => 3,
            "Itf4" => 4,
            "Itf3" => 5,
            other => bail!("Unknown indirect format {other}"),
        };
        let amount = if shift == 0 { -128 } else { 1 };
        let bias = match attr(ind, "BiasSel")? {
            "ItbNone" => [0, 0, 0],
            "ItbS" => [amount, 0, 0],
            "ItbT" => [0, amount, 0],
            "ItbU" => [0, 0, amount],
            "ItbST" => [amount, amount, 0],
            "ItbSU" => [amount, 0, amount],
            "ItbTU" => [0, amount, amount],
            "ItbSTU" => [amount; 3],
            other => bail!("Unknown indirect bias {other}"),
        };
        code.push_str(&format!("ivec3 bw_sample=ivec3(floor(clamp(textureProj(u_tex[{source_slot}],{source_coordinate}).abg,0.0,1.0)*255.0+0.5));bw_sample=(bw_sample>>{shift})+ivec3({},{},{});\n",bias[0],bias[1],bias[2]));
        let product = |row: usize| {
            format!("((bw_indirect_override?bw_indirect[{}].x:{})*bw_sample.x+(bw_indirect_override?bw_indirect[{}].y:{})*bw_sample.y+(bw_indirect_override?bw_indirect[{}].z:{})*bw_sample.z)",index*2+row,m[row][0],index*2+row,m[row][1],index*2+row,m[row][2])
        };
        code.push_str(&format!(
            "bw_offset=(ivec2({},{})>>3)<<1;\n",
            product(0),
            product(1)
        ));
    }
    let s = wrap("bw_base.x", attr(ind, "WrapS")?)?;
    let t = wrap("bw_base.y", attr(ind, "WrapT")?)?;
    let assignment = match attr(ind, "AddPrev")? {
        "true" => "+=",
        "false" => "=",
        other => bail!("Invalid indirect AddPrev {other}"),
    };
    code.push_str(&format!(
        "bw_ind_coord{assignment}ivec2({s},{t})+bw_offset;bw_ind_coord=(bw_ind_coord<<8)>>8;\n"
    ));
    ensure!(
        attr(ind, "UtcLod")? == "false",
        "Unmodified indirect LOD derivatives are not yet reproduced"
    );
    code.push_str(&format!("stage_tex[BW_STAGE]=texture(u_tex[{slot}],vec2(bw_ind_coord)/(vec2(textureSize(u_tex[{slot}],0))*128.0));}}\n"));
    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gx_packing_truncates_masks_and_sign_extends() -> Result<()> {
        assert_eq!(coefficient(0.05)?, 51);
        assert_eq!(coefficient(-0.05)?, -51);
        assert_eq!(coefficient(1.)?, -1024);
        assert_eq!(coefficient(2.)?, 0);
        assert_eq!(coefficient(-0.0009)?, 0);
        assert!(coefficient(f32::NAN).is_err());
        Ok(())
    }
}
