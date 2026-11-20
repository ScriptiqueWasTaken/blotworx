//! EM1 material vertex-property selection and GX raster output arithmetic.
//! Supporting hardware reference: Dolphin VideoCommon/LightingShaderGen.cpp,
//! GenerateLightingShaderHeader (clamped integer accumulator modulation).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaterialVertexState {
    /// The replacement property, not the original NIF property. DOL defaults
    /// 0x80767ad4 and 0x80767ae4 hold 0x08 and 0x18 respectively.
    pub property_flags: u16,
    /// Value written into mesh +0xd1 bit 3. This is not the GX channel's
    /// lighting enable; that also depends on renderer light state.
    pub mesh_flag: bool,
}

/// REL 0x16b82c..0x16b918; default creation DOL 0x804c269c..0x804c2840.
/// Template support defaults true (REL 0x166784). The fallback checks SOURCE
/// bits 4..5, not the lighting bit (3). Both installed defaults retain bit 3.
/// XML Channel attributes do not participate (REL 0x168114).
pub fn material_vertex_state(
    supports_hardware: bool,
    r3mt_flags: Option<i32>,
    vertex_flags: Option<u16>,
) -> MaterialVertexState {
    let selected = if supports_hardware && r3mt_flags.is_some() {
        r3mt_flags.unwrap() & 2 != 0
    } else {
        vertex_flags.is_some_and(|flags| (flags >> 4) & 3 == 1)
    };
    MaterialVertexState {
        property_flags: if selected { 0x18 } else { 0x08 },
        mesh_flag: selected,
    }
}

/// Material channels are bytes; the accumulated ambient/light sum may be
/// negative or exceed 255. Saturation happens before material modulation.
pub fn modulate(material: [u8; 4], accumulated: [i32; 4]) -> [u8; 4] {
    std::array::from_fn(|i| {
        let light = accumulated[i].clamp(0, 255) as u32;
        ((u32::from(material[i]) * (light + (light >> 7))) >> 8) as u8
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_selection_distinguishes_source_from_lighting_enable() {
        let selected = MaterialVertexState {
            property_flags: 0x18,
            mesh_flag: true,
        };
        let ordinary = MaterialVertexState {
            property_flags: 0x08,
            mesh_flag: false,
        };
        // Enabling lighting alone must not select the alternative property.
        assert_eq!(material_vertex_state(true, None, Some(0x08)), ordinary);
        assert_eq!(material_vertex_state(true, None, Some(0x10)), selected);
        assert_eq!(material_vertex_state(true, None, Some(0x18)), selected);
        // Original source mode 2 and unrelated bits are replaced, not retained.
        assert_eq!(material_vertex_state(true, None, Some(0xffef)), ordinary);
        assert_eq!(material_vertex_state(true, None, None), ordinary);
        // Mickey's actual R3MTLightingFlags=3 selects 0x18 without a property.
        assert_eq!(material_vertex_state(true, Some(3), None), selected);
        assert_eq!(material_vertex_state(true, Some(0), Some(0x18)), ordinary);
        assert_eq!(material_vertex_state(false, Some(0), Some(0x18)), selected);
        assert_eq!(material_vertex_state(false, Some(3), None), ordinary);
    }
    #[test]
    fn full_light_preserves_all_material_bytes_and_saturates_before_modulation() {
        for value in 0..=255 {
            assert_eq!(modulate([value; 4], [255; 4]), [value; 4]);
            assert_eq!(modulate([value; 4], [i32::MAX; 4]), [value; 4]);
            assert_eq!(modulate([value; 4], [i32::MIN; 4]), [0; 4]);
        }
        assert_eq!(modulate([255; 4], [0, 127, 128, 255]), [0, 126, 128, 255]);
        assert_eq!(modulate([64, 128, 192, 255], [128; 4]), [32, 64, 96, 128]);
    }
}
