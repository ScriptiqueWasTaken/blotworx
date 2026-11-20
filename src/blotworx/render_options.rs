//! Scene renderer controls verified in the EM1 REL. The silhouette shader
//! and mesh-enrollment lifecycle remain separate required rendering work.
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct State {
    pub draw_silhouettes: bool,
}
impl Default for State {
    // Constructor15bb78..88 stores1 into renderer byte29.
    fn default() -> Self {
        Self {
            draw_silhouettes: true,
        }
    }
}
impl State {
    /// Lua3a260 delegates to15e960, a byte store without rendering callbacks.
    pub fn set_draw_silhouettes(&mut self, enabled: bool) {
        self.draw_silhouettes = enabled;
    }
    ///15c200..21c enrolls runtimeD0 bit40;15c9c4..cc gates the extra draw phase.
    pub fn admits_silhouette(&self, flags: super::render_object::Flags) -> bool {
        self.draw_silhouettes && flags.is_silhouette()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_gate_does_not_admit_ordinary_meshes_or_serialized_av_flags() {
        let mut state = State::default();
        assert!(state.draw_silhouettes);
        let normal = super::super::render_object::Flags::from_native_byte(0x20);
        let silhouette = super::super::render_object::Flags::from_native_byte(0x60);
        assert!(!state.admits_silhouette(normal));
        assert!(state.admits_silhouette(silhouette));
        state.set_draw_silhouettes(false);
        assert!(!state.admits_silhouette(silhouette));
        assert!(silhouette.ignore_color_override());
        state.set_draw_silhouettes(true);
        assert!(state.admits_silhouette(silhouette));
    }
}
