//! PC32 little-endian hkpMotion fields used by EM2 body admission.
//!
//! Unlike the older Wii ABI, damping occupies two hkHalf fields and speed
//! limits live at +ba/+bb. Never endian-swap this record as an array of words.
//! Evidence: original PC resources and the EM2 Wii U hkpMotion constructors,
//! hkpRigidBody::getCinfo and createMotion. The original bytes remain owned by
//! the containing Body; decoding does not rewrite a packfile or motion payload.
use anyhow::{ensure, Result};

#[derive(Clone, Debug, serde::Serialize)]
pub struct Motion {
    pub motion_type: u8,
    pub transform: [f32; 16],
    pub previous_center: [f32; 4],
    pub center: [f32; 4],
    pub previous_orientation: [f32; 4],
    pub orientation: [f32; 4],
    pub local_center: [f32; 4],
    pub delta_angle: [f32; 4],
    pub object_radius: f32,
    pub linear_damping: f32,
    pub angular_damping: f32,
    /// Retain this separate packed field until its runtime semantics are verified.
    pub state_halfword_b8: u16,
    /// Havok 2012 hkUFloat8 codes, NOT indexes into the EM1 decoder's table.
    pub velocity_limit_codes: [u8; 2],
    pub velocity_limits: [f32; 2],
    pub deactivation_class: u8,
    pub inverse_inertia_mass: [f32; 4],
    pub linear_velocity: [f32; 4],
    pub angular_velocity: [f32; 4],
    pub gravity_factor: f32,
}
impl Motion {
    /// Component LoadRigidBodyFile mass stage (Wii U reference 4066158).
    /// A publication is returned synchronously for the next body to read.
    pub fn initialize_mass(&mut self, requested: f32) -> Result<Option<f32>> {
        use super::body_mass_properties::{self, Change, CharacterZero};
        match body_mass_properties::initialize(
            self.motion_type,
            self.inverse_inertia_mass,
            requested,
            CharacterZero::Havok2012,
        )? {
            Change::Unchanged => Ok(None),
            Change::Publish(value) => Ok(Some(value)),
            Change::Updated(values) => {
                self.inverse_inertia_mass = values;
                Ok(None)
            }
        }
    }
    /// Native loader clears inverse-inertia lanes, not angular velocity.
    /// The source's sphere-to-box conversion is selected by Y/Z restrictions;
    /// do not replace that branch with an inferred 'any axis' condition.
    pub fn restrict_rotation(&mut self, axes: [f32; 3]) -> Result<()> {
        ensure!(
            axes.iter().all(|v| v.is_finite()),
            "Nonfinite rotation restriction"
        );
        for (axis, value) in axes.into_iter().enumerate() {
            if value != 0. {
                self.inverse_inertia_mass[axis] = 0.;
            }
        }
        if self.motion_type == 2 && (axes[1] != 0. || axes[2] != 0.) {
            self.motion_type = 3;
        }
        Ok(())
    }
    /// Runs after placement/virtual initialization. Zero retains the imported
    /// value; damping and gravity writes truncate to hkHalf's high float word.
    pub fn initialize_tuning(
        &mut self,
        mut property: impl FnMut(&str) -> Result<f32>,
    ) -> Result<()> {
        for (index, name) in [
            "Linear Damping",
            "Angular Damping",
            "Gravity Factor",
            "Max Linear Velocity",
            "Max Angular Velocity",
        ]
        .into_iter()
        .enumerate()
        {
            let value = property(name)?;
            ensure!(value.is_finite(), "Nonfinite motion property {name}");
            if value == 0. {
                continue;
            }
            let half = f32::from_bits(value.to_bits() & 0xffff0000);
            match index {
                0 => self.linear_damping = half,
                1 => self.angular_damping = half,
                2 => self.gravity_factor = half,
                3 | 4 => {
                    let axis = index - 3;
                    let code = encode_velocity_limit(value)?;
                    self.velocity_limit_codes[axis] = code;
                    self.velocity_limits[axis] = decode_velocity_limit(code);
                }
                _ => unreachable!(),
            }
        }
        Ok(())
    }
}

pub fn decode(bytes: &[u8]) -> Result<Motion> {
    ensure!(
        bytes.len() == 0x120,
        "Invalid Havok 2012 motion record length"
    );
    let word = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let half = |at| u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap());
    let scalar = |at| f32::from_bits(word(at));
    let vector = |at| std::array::from_fn(|i| scalar(at + i * 4));
    let motion = Motion {
        motion_type: bytes[8],
        transform: std::array::from_fn(|i| scalar(0x10 + i * 4)),
        previous_center: vector(0x50),
        center: vector(0x60),
        previous_orientation: vector(0x70),
        orientation: vector(0x80),
        local_center: vector(0x90),
        delta_angle: vector(0xa0),
        object_radius: scalar(0xb0),
        // hkHalf stores the high 16 bits of an f32, not IEEE binary16.
        linear_damping: f32::from_bits(u32::from(half(0xb4)) << 16),
        angular_damping: f32::from_bits(u32::from(half(0xb6)) << 16),
        state_halfword_b8: half(0xb8),
        velocity_limit_codes: [bytes[0xba], bytes[0xbb]],
        velocity_limits: [
            decode_velocity_limit(bytes[0xba]),
            decode_velocity_limit(bytes[0xbb]),
        ],
        deactivation_class: bytes[0xbc],
        inverse_inertia_mass: vector(0xc0),
        linear_velocity: vector(0xd0),
        angular_velocity: vector(0xe0),
        gravity_factor: f32::from_bits(u32::from(half(0x11e)) << 16),
    };
    ensure!(
        (1..=7).contains(&motion.motion_type),
        "Invalid Havok 2012 motion type"
    );
    ensure!(
        motion.transform[15] == 1.,
        "Invalid Havok 2012 transform padding"
    );
    ensure!(
        motion
            .transform
            .iter()
            .chain(motion.previous_center.iter())
            .chain(motion.center.iter())
            .chain(motion.previous_orientation.iter())
            .chain(motion.orientation.iter())
            .chain(motion.local_center.iter())
            .chain(motion.delta_angle.iter())
            .chain(motion.linear_velocity.iter())
            .chain(motion.angular_velocity.iter())
            .chain(
                [
                    motion.object_radius,
                    motion.linear_damping,
                    motion.angular_damping,
                    motion.gravity_factor
                ]
                .iter()
            )
            .all(|v| v.is_finite()),
        "Nonfinite Havok 2012 motion data"
    );
    if matches!(motion.motion_type, 2 | 3 | 6 | 7) {
        ensure!(
            motion
                .inverse_inertia_mass
                .iter()
                .all(|v| v.is_finite() && *v >= 0.),
            "Invalid Havok 2012 dynamic inverse inertia/mass"
        );
    }
    Ok(motion)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> [u8; 0x120] {
        let mut bytes = [0; 0x120];
        bytes[8] = 3;
        for i in [0, 5, 10, 15] {
            bytes[0x10 + i * 4..0x14 + i * 4].copy_from_slice(&1f32.to_le_bytes());
        }
        bytes[0xb4..0xc0].copy_from_slice(&[0, 0, 0x4d, 0x3d, 0x80, 0x3f, 0x7f, 0x7f, 2, 0, 0, 0]);
        bytes[0x11e..].copy_from_slice(&0x3f80u16.to_le_bytes());
        bytes
    }
    #[test]
    fn packed_fields_keep_their_original_lane_order() {
        let mut bytes = fixture();
        bytes[0xba] = 0x21;
        bytes[0xbb] = 0x98;
        bytes[0xd0..0xd4].copy_from_slice(&(-3.5f32).to_le_bytes());
        bytes[0xc4..0xc8].copy_from_slice(&17.25f32.to_le_bytes());
        let original = bytes;
        let motion = decode(&bytes).unwrap();
        assert_eq!(motion.linear_damping.to_bits(), 0);
        assert_eq!(motion.angular_damping.to_bits(), 0x3d4d0000);
        assert_eq!(motion.state_halfword_b8, 0x3f80);
        assert_eq!(motion.velocity_limit_codes, [0x21, 0x98]);
        assert_eq!(motion.deactivation_class, 2);
        assert_eq!(motion.linear_velocity[0], -3.5);
        assert_eq!(motion.inverse_inertia_mass[1], 17.25);
        assert_eq!(motion.gravity_factor, 1.);
        assert_eq!(bytes, original);
    }
    #[test]
    fn pc_limits_use_the_original_compressed_table() {
        assert_eq!(decode_velocity_limit(0).to_bits(), 0);
        assert_eq!(decode_velocity_limit(1).to_bits(), 0x3c23d000);
        assert_eq!(decode_velocity_limit(127), 202.8125);
        assert_eq!(decode_velocity_limit(255), 999936.);
        assert!((0..255).all(|i| decode_velocity_limit(i) < decode_velocity_limit(i + 1)));
        for code in 0..=255u8 {
            let value = decode_velocity_limit(code);
            assert_eq!(encode_velocity_limit(value).unwrap(), code);
            if code > 0 {
                assert_eq!(
                    encode_velocity_limit(f32::from_bits(value.to_bits() - 1)).unwrap(),
                    if code == 1 { 0 } else { code }
                );
                assert_eq!(
                    encode_velocity_limit(f32::from_bits(value.to_bits() + 1)).unwrap(),
                    code.saturating_add(1)
                );
            }
        }
        assert_eq!(encode_velocity_limit(-20.).unwrap(), 0);
        assert_eq!(encode_velocity_limit(f32::MAX).unwrap(), 255);
        assert!(encode_velocity_limit(f32::NAN).is_err());
    }
    #[test]
    fn tuning_uses_packed_writes_and_preserves_prior_changes_on_failure() -> Result<()> {
        let mut motion = decode(&fixture())?;
        let before = motion.angular_damping;
        let mut reads = Vec::new();
        assert!(motion
            .initialize_tuning(|name| {
                reads.push(name.to_owned());
                if name == "Linear Damping" {
                    Ok(0.123456)
                } else {
                    anyhow::bail!("property read failed")
                }
            })
            .is_err());
        assert_eq!(reads, ["Linear Damping", "Angular Damping"]);
        assert_eq!(
            motion.linear_damping.to_bits(),
            0.123456f32.to_bits() & 0xffff0000
        );
        assert_eq!(motion.angular_damping, before);
        motion.initialize_tuning(|name| {
            Ok(match name {
                "Max Linear Velocity" => 200.,
                "Max Angular Velocity" => -1.,
                "Gravity Factor" => 0.5,
                _ => 0.,
            })
        })?;
        assert_eq!(motion.velocity_limits, [202.8125, 0.]);
        assert_eq!(motion.gravity_factor, 0.5);
        let mut sphere = motion.clone();
        sphere.motion_type = 2;
        sphere.inverse_inertia_mass = [3., 3., 3., 1.];
        sphere.angular_velocity = [1., 2., 3., 0.];
        sphere.restrict_rotation([1.; 3])?;
        assert_eq!(sphere.motion_type, 3);
        assert_eq!(sphere.inverse_inertia_mass, [0., 0., 0., 1.]);
        assert_eq!(sphere.angular_velocity, [1., 2., 3., 0.]);
        Ok(())
    }
    #[test]
    fn malformed_records_fail_before_admission() {
        let source = fixture();
        assert!(decode(&source[..0x11f]).is_err());
        for (at, value) in [
            (8, 8u32),
            (0xc0, f32::NAN.to_bits()),
            (0xd0, f32::INFINITY.to_bits()),
            (0x4c, 0),
        ] {
            let mut invalid = source;
            invalid[at..at + 4].copy_from_slice(&value.to_le_bytes());
            assert!(decode(&invalid).is_err(), "Accepted invalid field {at:x}");
        }
    }
}

/// PC getCinfo at 0x80181f reads a u16, adds 0x3b800 and shifts left 12;
/// table zero is special. The older Wii float table is not interchangeable.
pub fn decode_velocity_limit(code: u8) -> f32 {
    let word = VELOCITY_LIMIT_WORDS[code as usize];
    f32::from_bits(if word == 0 {
        0
    } else {
        (u32::from(word) + 0x3b800) << 12
    })
}
/// PC hkUFloat8::operator= at 0xdf99e0..0xdf9a9e: under-minimum zero,
/// otherwise ascending-table ceiling with saturation. Nonfinite inputs need
/// separate native diagnostics and are not silently mapped to a limit.
pub fn encode_velocity_limit(value: f32) -> Result<u8> {
    ensure!(value.is_finite(), "Nonfinite Havok 2012 velocity limit");
    if value < decode_velocity_limit(1) {
        return Ok(0);
    }
    let index = VELOCITY_LIMIT_WORDS.partition_point(|&word| {
        let decoded = f32::from_bits(if word == 0 {
            0
        } else {
            (u32::from(word) + 0x3b800) << 12
        });
        decoded < value
    });
    Ok(index.min(255) as u8)
}

// DEM2_unpacked.exe SHA256 80fdf5ea01fdde0e0e640bb0bade4b8af6c90686ed78f51473870b734b8056cb
// Table VA 0x1266640, file offset 0xe64640, 256 original little-endian u16s.
const VELOCITY_LIMIT_WORDS: [u16; 256] = [
    0, 2621, 4669, 5980, 6717, 7372, 8028, 8437, 8595, 8763, 8943, 9135, 9341, 9561, 9795, 10046,
    10277, 10420, 10573, 10737, 10912, 11098, 11298, 11511, 11739, 11983, 12244, 12405, 12553,
    12712, 12882, 13064, 13258, 13465, 13687, 13923, 14176, 14391, 14536, 14690, 14855, 15031,
    15220, 15421, 15637, 15867, 16112, 16375, 16520, 16670, 16830, 17001, 17185, 17380, 17589,
    17813, 18052, 18307, 18506, 18651, 18807, 18974, 19152, 19342, 19545, 19762, 19994, 20242,
    20493, 20635, 20786, 20948, 21121, 21306, 21503, 21714, 21939, 22180, 22438, 22620, 22767,
    22924, 23092, 23272, 23464, 23669, 23888, 24122, 24372, 24607, 24750, 24903, 25066, 25240,
    25427, 25626, 25839, 26066, 26309, 26569, 26735, 26883, 27042, 27211, 27392, 27586, 27793,
    28014, 28250, 28502, 28722, 28866, 29020, 29184, 29360, 29548, 29749, 29964, 30193, 30438,
    30701, 30850, 31000, 31160, 31330, 31513, 31708, 31917, 32140, 32378, 32633, 32836, 32981,
    33137, 33303, 33480, 33670, 33873, 34089, 34321, 34568, 34824, 34965, 35116, 35277, 35450,
    35634, 35831, 36041, 36266, 36507, 36763, 36951, 37097, 37254, 37422, 37601, 37792, 37996,
    38215, 38448, 38698, 38938, 39080, 39233, 39395, 39569, 39755, 39954, 40166, 40393, 40635,
    40895, 41065, 41213, 41372, 41540, 41721, 41914, 42120, 42341, 42576, 42828, 43052, 43196,
    43350, 43514, 43689, 43877, 44077, 44291, 44520, 44765, 45026, 45180, 45330, 45489, 45660,
    45842, 46036, 46244, 46467, 46704, 46958, 47167, 47312, 47467, 47632, 47809, 47998, 48200,
    48416, 48647, 48894, 49155, 49295, 49446, 49607, 49779, 49963, 50159, 50369, 50593, 50833,
    51089, 51281, 51427, 51584, 51751, 51929, 52120, 52324, 52542, 52775, 53024, 53269, 53411,
    53563, 53725, 53898, 54084, 54282, 54494, 54720, 54962, 55220, 55396, 55543, 55701, 55870,
    56050, 56242, 56448, 56668, 56903, 57154,
];
