//! Verified vertex-color roles for materials using the diffuse preview path.
//! Color attributes remain intact: a shader may use them as vertex-stage masks.

pub(super) fn modulates_diffuse(shader: Option<&str>) -> bool {
    // Original PC Gen3_Char_Mickey.fxo: MickeyEarSkinning.fxh reads COLOR0.g
    // to select ear transforms. The pixel shader has no COLOR interpolation;
    // its diffuse sample is not multiplied by that mask.
    !shader.is_some_and(|name| name.eq_ignore_ascii_case("Gen3_Char_Mickey"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mask_role_is_shader_specific_and_case_insensitive() {
        assert!(!modulates_diffuse(Some("Gen3_Char_Mickey")));
        assert!(!modulates_diffuse(Some("gen3_char_mickey")));
        assert!(modulates_diffuse(None));
        assert!(modulates_diffuse(Some("GEN_Diffuse_VertexColor")));
        assert!(modulates_diffuse(Some("UnknownMaterial")));
    }
}
