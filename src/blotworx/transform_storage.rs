//! Owned JPSTransformationComponent storage projection. Typed wrappers have
//! a four-byte header; setters3313cc/399abc/123ec4 don't notify by themselves.
use super::{native_matrix::Matrix, native_transform_record::Transform};
use anyhow::{ensure, Result};
#[derive(Clone, Copy)]
pub enum Space {
    Local,
    World,
}
pub struct Storage<'a> {
    bytes: &'a mut [u8],
}
pub fn world_word(bytes: &[u8], index: usize) -> Result<u32> {
    let offset = match index {
        0..=8 => 0x84 + index * 4,
        9..=11 => 0x74 + (index - 9) * 4,
        12 => 0xac,
        _ => anyhow::bail!("Invalid world transform word {index}"),
    };
    let word = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| anyhow::anyhow!("Truncated world transform word {index}"))?;
    Ok(u32::from_be_bytes(word.try_into().unwrap()))
}
impl<'a> Storage<'a> {
    /// Borrow caller-owned initialized component bytes; never fabricate defaults.
    pub fn new(bytes: &'a mut [u8]) -> Result<Self> {
        ensure!(
            bytes.len() >= 0x13c,
            "Truncated transformation component storage"
        );
        Ok(Self { bytes })
    }
    fn read<const N: usize>(&self, offset: usize) -> [f32; N] {
        std::array::from_fn(|i| {
            f32::from_bits(u32::from_be_bytes(
                self.bytes[offset + i * 4..offset + i * 4 + 4]
                    .try_into()
                    .unwrap(),
            ))
        })
    }
    fn write<const N: usize>(&mut self, offset: usize, value: [f32; N]) {
        for (i, v) in value.into_iter().enumerate() {
            self.bytes[offset + i * 4..offset + i * 4 + 4]
                .copy_from_slice(&v.to_bits().to_be_bytes());
        }
    }
    pub fn position(&self, space: Space) -> [f32; 3] {
        self.read(match space {
            Space::Local => 0x34,
            Space::World => 0x74,
        })
    }
    pub fn store_position(&mut self, space: Space, value: [f32; 3]) {
        self.write(
            match space {
                Space::Local => 0x34,
                Space::World => 0x74,
            },
            value,
        )
    }
    pub fn rotation(&self, space: Space) -> Matrix {
        self.read(match space {
            Space::Local => 0x44,
            Space::World => 0x84,
        })
    }
    pub fn store_rotation(&mut self, space: Space, value: Matrix) {
        self.write(
            match space {
                Space::Local => 0x44,
                Space::World => 0x84,
            },
            value,
        )
    }
    pub fn world_scale(&self) -> f32 {
        self.read::<1>(0xac)[0]
    }
    pub fn store_world_scale(&mut self, value: f32) {
        self.write(0xac, [value]);
    }
    pub fn cached_parent(&self) -> Transform {
        self.read(0xe8)
    }
    pub fn store_cached_parent(&mut self, value: Transform) {
        self.write(0xe8, value);
    }
    pub fn store_cache_word(&mut self, index: usize, word: u32) -> Result<()> {
        ensure!(index < 13, "Invalid cached transform word {index}");
        let offset = 0xe8 + index * 4;
        self.bytes[offset..offset + 4].copy_from_slice(&word.to_be_bytes());
        Ok(())
    }
    pub fn byte(&self, offset: usize) -> Result<u8> {
        ensure!(
            matches!(offset, 0x20 | 0x124 | 0x125 | 0x126 | 0x127 | 0x128),
            "Unsupported transform flag offset {offset:x}"
        );
        Ok(self.bytes[offset])
    }
    pub fn store_byte(&mut self, offset: usize, value: u8) -> Result<()> {
        ensure!(
            matches!(offset, 0x124 | 0x125 | 0x126 | 0x127 | 0x128),
            "Unsupported transform flag store {offset:x}"
        );
        self.bytes[offset] = value;
        Ok(())
    }
    pub fn related(&self, offset: usize) -> Result<Option<usize>> {
        ensure!(
            matches!(offset, 0x134 | 0x138),
            "Unsupported related transform record offset {offset:x}"
        );
        let token = u32::from_be_bytes(self.bytes[offset..offset + 4].try_into().unwrap());
        Ok((token != 0).then_some(token as usize))
    }
    fn word(&self, offset: usize) -> u32 {
        u32::from_be_bytes(self.bytes[offset..offset + 4].try_into().unwrap())
    }
    pub fn parent_entity(&self) -> u32 {
        self.word(0xb4)
    }
    pub fn cache_record(&self) -> usize {
        self.word(0x11c) as usize
    }
    pub fn attachment_mode(&self) -> i32 {
        self.word(0xc4) as i32
    }
    /// Getter353af8/3313e8: Vector3 wrapper at+c8, value at+cc.
    pub fn attachment_offset(&self) -> [f32; 3] {
        self.read(0xcc)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_storage_retains_headers_and_has_one_pose_authority() -> Result<()> {
        let mut owner = [0xa5; 0x144];
        {
            let mut s = Storage::new(&mut owner)?;
            s.store_position(Space::Local, [1., 2., 3.]);
            s.store_position(Space::World, [4., 5., 6.]);
            let r = [1., 2., 3., 4., 5., 6., 7., 8., 9.];
            s.store_rotation(Space::Local, r);
            s.store_rotation(Space::World, r.map(|v| v + 10.));
            s.store_world_scale(-0.);
            assert_eq!(s.world_scale().to_bits(), 0x80000000);
            assert_eq!(s.position(Space::Local), [1., 2., 3.]);
            assert_eq!(s.position(Space::World), [4., 5., 6.]);
            assert_eq!(s.rotation(Space::Local), r);
            assert_eq!(s.rotation(Space::World), r.map(|v| v + 10.));
            let parent = [0., 1., 0., -1., 0., 0., 0., 0., 1., 2., 4., 6., 2.];
            s.store_cached_parent(parent);
            assert_eq!(s.cached_parent(), parent);
        }
        for header in [0x30, 0x40, 0x70, 0x80, 0xa8] {
            assert_eq!(&owner[header..header + 4], &[0xa5; 4]);
        }
        assert!(owner[0xb0..0xe8].iter().all(|b| *b == 0xa5));
        assert!(owner[0x11c..].iter().all(|b| *b == 0xa5));
        let mut short = [0x5a; 0x13b];
        assert!(Storage::new(&mut short).is_err());
        assert!(short.iter().all(|b| *b == 0x5a));
        Ok(())
    }
}
