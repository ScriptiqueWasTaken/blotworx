//! Light-range queries and directional shadow-source selection.
use super::viewport3d::GpuLight;
pub fn intersects(light: &GpuLight, lo: [f32; 3], hi: [f32; 3]) -> bool {
    if light.dimmer == 0. {
        return false;
    }
    if light.range > 0. {
        let distance2 = (0..3)
            .map(|i| {
                let d = (lo[i] - light.position[i])
                    .max(0.)
                    .max(light.position[i] - hi[i]);
                d * d
            })
            .sum::<f32>();
        distance2 <= light.range * light.range
    } else {
        true
    }
}
pub fn key_directional(lights: &[GpuLight], eye: [f32; 3]) -> Option<usize> {
    lights
        .iter()
        .enumerate()
        .filter(|(_, l)| {
            l.light_type == 1 && l.dimmer > 0. && intersects(l, eye, eye) && score(l, eye) > 0.
        })
        .max_by(|(_, a), (_, b)| score(a, eye).total_cmp(&score(b, eye)))
        .map(|(i, _)| i)
}
fn score(l: &GpuLight, p: [f32; 3]) -> f32 {
    let d = (0..3)
        .map(|i| (p[i] - l.position[i]).powi(2))
        .sum::<f32>()
        .sqrt();
    let falloff = if l.range > 0. && d >= l.range_start {
        ((l.range - d) / (l.range - l.range_start).max(1e-6)).clamp(0., 1.)
    } else {
        1.
    };
    l.diffuse.iter().sum::<f32>() * l.dimmer * falloff
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finite_lights_cull_conservatively_even_for_directional_sources() {
        let mut l = GpuLight::default();
        l.position = [0.; 3];
        l.range = 3.;
        l.light_type = 1;
        assert!(intersects(&l, [2., 0., 0.], [4., 1., 1.]));
        assert!(!intersects(&l, [4., 0., 0.], [5., 1., 1.]));
        l.range = 0.;
        assert!(intersects(&l, [400., 0., 0.], [500., 1., 1.]));
    }
    #[test]
    fn key_shadow_light_must_reach_the_view() {
        let mut a = GpuLight::default();
        a.light_type = 1;
        a.range = 2.;
        a.diffuse = [1.; 3];
        a.position = [100., 0., 0.];
        let mut b = a.clone();
        b.position = [0.; 3];
        assert_eq!(key_directional(&[a, b], [0.; 3]), Some(1));
    }
}
