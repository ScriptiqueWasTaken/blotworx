//! Retail material curve sampling, REL 0x166938 and 0x17719c.
//! Asset decoding and live register binding remain separate integration work.
use anyhow::{ensure, Result};
#[derive(Clone, Debug)]
pub enum Curve {
    Absent,
    Constant(f32),
    Keys(Vec<[f32; 2]>),
}
impl Curve {
    pub fn sample(&self, time: f32) -> Result<Option<f32>> {
        ensure!(time.is_finite(), "Nonfinite material curve time");
        match self {
            Self::Absent => Ok(None),
            Self::Constant(value) => {
                ensure!(value.is_finite(), "Nonfinite material curve constant");
                Ok(Some(*value))
            }
            Self::Keys(keys) => {
                ensure!(
                    keys.len() >= 2,
                    "Native keyed material curve needs at least two keys"
                );
                ensure!(
                    keys.iter().flatten().all(|v| v.is_finite()),
                    "Nonfinite material curve key"
                );
                // Native linear search advances at equality: duplicate times
                // select the last equal key. Preserve authored ordering.
                let upper = keys
                    .iter()
                    .position(|key| time < key[0])
                    .unwrap_or(keys.len());
                if upper == 0 {
                    return Ok(Some(keys[0][1]));
                }
                if upper == keys.len() {
                    return Ok(Some(keys[upper - 1][1]));
                }
                let [t0, v0] = keys[upper - 1];
                let [t1, v1] = keys[upper];
                let span = t1 - t0;
                Ok(Some(if span == 0. {
                    v1
                } else {
                    (v1 - v0).mul_add((time - t0) / span, v0)
                }))
            }
        }
    }
    /// The transition call uses non-looping animation. Native converts seconds
    /// to frames at 60 Hz, clamps only above duration, then clamps/truncates bytes.
    pub fn transition_byte(&self, seconds: f32, duration_frames: f32) -> Result<Option<u8>> {
        ensure!(
            duration_frames.is_finite() && duration_frames >= 0.,
            "Invalid material animation duration"
        );
        ensure!(seconds.is_finite(), "Nonfinite material animation time");
        let frame = seconds * 60.;
        let frame = if frame > duration_frames {
            duration_frames
        } else {
            frame
        };
        Ok(self.sample(frame)?.map(|v| v.clamp(0., 255.) as u8))
    }
}
/// Original RCLA color channels. Unknown targets fail explicitly rather than
/// silently dropping animation the engine does not yet support.
#[derive(Debug)]
pub struct ColorAnimation {
    pub duration_frames: f32,
    pub channels: Vec<ColorChannel>,
}
#[derive(Debug)]
pub struct ColorChannel {
    pub constant: bool,
    pub register: usize,
    pub component: usize,
    pub curve: Curve,
}
impl ColorAnimation {
    pub fn parse(text: &str) -> Result<Self> {
        use anyhow::{bail, Context};
        let doc = roxmltree::Document::parse(text.trim_start_matches('\u{feff}'))?;
        let root = doc
            .descendants()
            .find(|n| n.has_tag_name("rcla"))
            .context("Missing RCLA animation")?;
        let info = root
            .children()
            .find(|n| n.has_tag_name("color_anim_info"))
            .context("Missing color animation info")?;
        let duration_frames = info
            .attribute("frame_size")
            .context("Missing animation duration")?
            .parse::<f32>()?;
        ensure!(
            duration_frames.is_finite() && duration_frames >= 0.,
            "Invalid animation duration"
        );
        let materials: Vec<_> = root
            .descendants()
            .filter(|n| n.has_tag_name("color_anim"))
            .collect();
        ensure!(
            materials.len() == 1,
            "Multiple/absent material animations need native overwrite semantics"
        );
        let mut channels = Vec::new();
        for target in materials[0]
            .children()
            .filter(|n| n.has_tag_name("color_anim_target"))
        {
            let name = target.attribute("target").context("Missing color target")?;
            let (constant, suffix) = if let Some(s) = name.strip_prefix("tev_constant") {
                (true, s)
            } else if let Some(s) = name.strip_prefix("tev_color") {
                (false, s)
            } else {
                bail!("Unsupported color animation target {name}");
            };
            let bytes = suffix.as_bytes();
            ensure!(
                bytes.len() == 3 && bytes[0].is_ascii_digit(),
                "Invalid color animation target {name}"
            );
            let register = (bytes[0] - b'0') as usize;
            ensure!(
                register < if constant { 4 } else { 3 },
                "Invalid color register {name}"
            );
            let component = match bytes[2] {
                b'r' => 0,
                b'g' => 1,
                b'b' => 2,
                b'a' => 3,
                _ => bail!("Invalid color component {name}"),
            };
            let curve = if let Some(value) =
                target.children().find(|n| n.has_tag_name("anim_constant"))
            {
                let value = value
                    .attribute("value")
                    .context("Missing constant value")?
                    .parse::<f32>()?;
                ensure!(value.is_finite(), "Nonfinite color constant");
                Curve::Constant(value)
            } else if let Some(value) = target.children().find(|n| n.has_tag_name("anim_hermite")) {
                let count = value
                    .attribute("size")
                    .context("Missing key count")?
                    .parse::<usize>()?;
                // Retail clamps to ten and discards tangents. Reject overflow
                // explicitly until an asset requiring truncation is validated.
                ensure!(
                    (2..=10).contains(&count),
                    "Unsupported native color key count {count}"
                );
                let values = value
                    .text()
                    .unwrap_or("")
                    .split_whitespace()
                    .map(str::parse::<f32>)
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                ensure!(
                    values.len() == count * 3 && values.iter().all(|v| v.is_finite()),
                    "Invalid color curve triples"
                );
                Curve::Keys(values.chunks_exact(3).map(|v| [v[0], v[1]]).collect())
            } else {
                Curve::Absent
            };
            channels.push(ColorChannel {
                constant,
                register,
                component,
                curve,
            });
        }
        ensure!(
            channels.len() <= 16,
            "Native color animation has only sixteen channel slots"
        );
        Ok(Self {
            duration_frames,
            channels,
        })
    }
    /// Update only animated components, preserving signed TEV defaults in all
    /// untouched channels. This array is consumed directly by the GPU draw.
    pub fn apply_uniforms(&self, seconds: f32, registers: &mut [[f32; 4]; 7]) -> Result<()> {
        for channel in &self.channels {
            if let Some(value) = channel
                .curve
                .transition_byte(seconds, self.duration_frames)?
            {
                registers[channel.register + if channel.constant { 3 } else { 0 }]
                    [channel.component] = value as f32 / 255.;
            }
        }
        Ok(())
    }
    pub fn apply_transition(
        &self,
        seconds: f32,
        colors: &mut [[u8; 4]; 3],
        constants: &mut [[u8; 4]; 4],
    ) -> Result<()> {
        for channel in &self.channels {
            if let Some(value) = channel
                .curve
                .transition_byte(seconds, self.duration_frames)?
            {
                if channel.constant {
                    constants[channel.register][channel.component] = value;
                } else {
                    colors[channel.register][channel.component] = value;
                }
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rcla_ignores_authored_tangents_and_preserves_other_register_channels() -> Result<()> {
        let text = r#"<nw4r_3dif><body><rcla><color_anim_info frame_size="90"/><color_anim_array><color_anim>
          <color_anim_target target="tev_constant0.g"><anim_hermite size="2">0 0 900 90 255 -900</anim_hermite></color_anim_target>
          <color_anim_target target="tev_color2.a"><anim_constant value="42.9"/></color_anim_target>
        </color_anim></color_anim_array></rcla></body></nw4r_3dif>"#;
        let animation = ColorAnimation::parse(text)?;
        let mut colors = [[9; 4]; 3];
        let mut constants = [[8; 4]; 4];
        animation.apply_transition(0.75, &mut colors, &mut constants)?;
        let mut uniforms = [[-2.; 4]; 7];
        animation.apply_uniforms(0.75, &mut uniforms)?;
        assert_eq!(uniforms[2], [-2., -2., -2., 42. / 255.]);
        assert_eq!(uniforms[3], [-2., 127. / 255., -2., -2.]);
        assert_eq!(colors[2], [9, 9, 9, 42]);
        assert_eq!(constants[0], [8, 127, 8, 8]);
        assert!(ColorAnimation::parse(&text.replace("tev_color2.a", "unknown")).is_err());
        Ok(())
    }
    #[test]
    #[ignore = "Requires original imported BLOTWORX_TEST_DATA"]
    fn original_transition_animation_inventory() -> Result<()> {
        use epic_mickey_lib_rs::packfile::Packfile;
        let root = std::path::PathBuf::from(std::env::var("BLOTWORX_TEST_DATA")?);
        let pak = Packfile::from_binary(std::fs::read(root.join("packfiles/r3mt.pak"))?);
        let mut count = 0;
        for file in &pak.files {
            let name = file.path.to_ascii_lowercase();
            if name.ends_with(".rcla")
                && (name.contains("painttosketch") || name.contains("sketchtopaint"))
            {
                let animation = ColorAnimation::parse(std::str::from_utf8(&file.data)?)?;
                println!(
                    "{}: duration={} channels={}",
                    file.path,
                    animation.duration_frames,
                    animation.channels.len()
                );
                count += 1;
            }
        }
        ensure!(count > 0, "No transition RCLA assets in r3mt.pak");
        Ok(())
    }
    #[test]
    fn native_constant_empty_and_duplicate_time_branches() -> Result<()> {
        assert_eq!(Curve::Absent.sample(1.)?, None);
        assert_eq!(Curve::Constant(17.).sample(-1.)?, Some(17.));
        let curve = Curve::Keys(vec![[0., 0.], [10., 100.], [10., 150.], [20., 200.]]);
        for (time, value) in [(-1., 0.), (5., 50.), (10., 150.), (15., 175.), (25., 200.)] {
            assert_eq!(curve.sample(time)?, Some(value));
        }
        Ok(())
    }
    #[test]
    fn animation_uses_sixty_hz_upper_clamp_and_truncation() -> Result<()> {
        let curve = Curve::Keys(vec![[0., 0.], [60., 255.]]);
        assert_eq!(curve.transition_byte(0.5, 60.)?, Some(127));
        assert_eq!(curve.transition_byte(2., 30.)?, Some(127));
        assert_eq!(Curve::Constant(-12.).transition_byte(0., 60.)?, Some(0));
        assert_eq!(Curve::Constant(300.).transition_byte(0., 60.)?, Some(255));
        assert!(curve.sample(f32::NAN).is_err());
        assert!(curve.transition_byte(0., f32::INFINITY).is_err());
        Ok(())
    }
}
