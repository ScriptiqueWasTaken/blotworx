//! Owned auxiliary state for the character constructor's runtime publication.
//! This does not allocate/install a rigid body or implement its motion/solver.
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;
pub struct BodyAuxiliary {
    pub properties: super::body_properties::Properties,
    pub listeners: super::body_listeners::BodyListeners,
    pub user_data: Option<super::collidable_owner::EntityData>,
}
pub trait BodyRecords {
    fn contains(&self, body: usize) -> bool;
    fn auxiliary(&mut self, body: usize) -> Option<&mut BodyAuxiliary>;
}
impl BodyRecords for BTreeMap<usize, BodyAuxiliary> {
    fn contains(&self, body: usize) -> bool {
        self.contains_key(&body)
    }
    fn auxiliary(&mut self, body: usize) -> Option<&mut BodyAuxiliary> {
        self.get_mut(&body)
    }
}
impl BodyRecords for BTreeMap<usize, super::dynamic_body::PlacedBody> {
    fn contains(&self, body: usize) -> bool {
        self.contains_key(&body)
    }
    fn auxiliary(&mut self, body: usize) -> Option<&mut BodyAuxiliary> {
        self.get_mut(&body).map(|body| &mut body.auxiliary)
    }
}
pub struct OwnedRuntime<'a, B: BodyRecords> {
    wrapper: &'a mut [u8],
    bodies: &'a mut B,
    listener: usize,
}
impl<'a, B: BodyRecords> OwnedRuntime<'a, B> {
    /// wrapper+10 is an existing opaque32-bit body token, not a host/Wii pointer.
    /// Wrapper bytes and body auxiliary records must come from their real owner.
    pub fn new(wrapper: &'a mut [u8], bodies: &'a mut B, listener: usize) -> Result<Self> {
        ensure!(
            wrapper.len() >= 0x64,
            "Truncated owned character wrapper storage"
        );
        ensure!(
            listener != 0,
            "Character wrapper listener interface is null"
        );
        Ok(Self {
            wrapper,
            bodies,
            listener,
        })
    }
    fn words(&mut self, offset: usize, words: &[u32]) {
        for (i, word) in words.iter().enumerate() {
            self.wrapper[offset + i * 4..offset + i * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
    }
    /// Evaluate the real wrapper's current +50 velocity/+60 response limit.
    /// This does not install the result into a solver; the contact owner must
    /// preserve the returned XYZ/W factors and contacted body order.
    pub fn contact_mass_factors(
        &self,
        other_motion: u8,
        character_is_body_a: bool,
        normals: &[[f32; 4]],
        inverse_mass: f32,
        fallback: [f32; 4],
    ) -> Result<Option<[f32; 4]>> {
        let scalar =
            |offset| f32::from_be_bytes(self.wrapper[offset..offset + 4].try_into().unwrap());
        super::character_contact_response::mass_factors(
            other_motion,
            character_is_body_a,
            normals,
            std::array::from_fn(|i| scalar(0x50 + 4 * i)),
            inverse_mass,
            scalar(0x60),
            fallback,
        )
    }
}
impl<B: BodyRecords> super::character_body_info::RuntimeBackend for OwnedRuntime<'_, B> {
    fn body(&mut self) -> Result<usize> {
        let body = u32::from_be_bytes(self.wrapper[0x10..0x14].try_into().unwrap()) as usize;
        ensure!(
            body != 0 && self.bodies.contains(body),
            "Character body token has no owned auxiliary record"
        );
        Ok(body)
    }
    fn property(&mut self, body: usize, key: u32, bits: u64) -> Result<()> {
        self.bodies
            .auxiliary(body)
            .context("Body disappeared during property installation")?
            .properties
            .add(key, bits)?;
        Ok(())
    }
    fn up(&mut self, words: [u32; 4]) -> Result<()> {
        self.words(0x20, &words);
        Ok(())
    }
    fn scalar(&mut self, offset: usize, word: u32) -> Result<()> {
        ensure!(
            matches!(offset, 0x30 | 0x34 | 0x38 | 0x3c | 0x40 | 0x60),
            "Unimplemented character scalar offset {offset:x}"
        );
        self.words(offset, &[word]);
        Ok(())
    }
    fn velocity(&mut self, words: [u32; 4]) -> Result<()> {
        self.words(0x50, &words);
        Ok(())
    }
    fn register_body_listener(&mut self, body: usize) -> Result<()> {
        self.bodies
            .auxiliary(body)
            .context("Body disappeared during listener registration")?
            .listeners
            .register(Some(self.listener))?;
        Ok(())
    }
    fn clear_pointer_14(&mut self) -> Result<()> {
        self.words(0x14, &[0]);
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn contact_response_reads_current_owned_wrapper_fields() -> Result<()> {
        let mut wrapper = [0; 0x64];
        let mut bodies = BTreeMap::<usize, BodyAuxiliary>::new();
        let mut runtime = OwnedRuntime::new(&mut wrapper, &mut bodies, 1)?;
        runtime.words(0x50, &[4f32.to_bits(), 0, 0, 0]);
        runtime.words(0x60, &[50f32.to_bits()]);
        let normals = [[1., 0., 0., 0.], [1., 0., 0., 0.]];
        assert_eq!(
            runtime.contact_mass_factors(3, false, &normals, 0.01, [0.; 4])?,
            Some([1., 1., 0.001, 4.])
        );
        runtime.words(0x50, &[8f32.to_bits(), 0, 0, 0]);
        assert_eq!(
            runtime.contact_mass_factors(3, false, &normals, 0.01, [0.; 4])?,
            Some([1., 1., 0.001, 8.])
        );
        runtime.words(0x60, &[100f32.to_bits()]);
        assert_eq!(
            runtime.contact_mass_factors(3, false, &normals, 0.01, [0.; 4])?,
            Some([1., 1., 0.001, 4.])
        );
        assert!(
            bodies.is_empty(),
            "Response evaluation must not synthesize body records"
        );
        Ok(())
    }
    #[test]
    fn placed_body_projection_publishes_into_the_existing_owner_without_copying() -> Result<()> {
        let mut record = super::super::native_motion_record::MotionRecord([0; 0x120]);
        record.0[8] = 7;
        let motion = super::super::body_motion::BodyMotion::new(record, 8, None)?;
        let auxiliary = BodyAuxiliary {
            properties: Default::default(),
            listeners: Default::default(),
            user_data: Some(super::super::collidable_owner::EntityData::character(
                Some(71),
                5,
            )),
        };
        let body = super::super::dynamic_body::PlacedBody::new(motion, vec![], auxiliary)?;
        let mut bodies: BTreeMap<usize, _> = BTreeMap::from([(7, body)]);
        let owner_address = &bodies[&7].auxiliary as *const BodyAuxiliary;
        let original_motion = bodies[&7].motion.current.clone();
        let mut wrapper = [0xa5; 0x70];
        wrapper[0x10..0x14].copy_from_slice(&7u32.to_be_bytes());
        let mut source = [0; 0x70];
        source[0x68..0x6c].copy_from_slice(&0xffffffffu32.to_be_bytes());
        super::super::character_body_info::initialize_runtime(
            &source,
            [0; 2],
            &mut OwnedRuntime::new(&mut wrapper, &mut bodies, 100)?,
        )?;
        assert_eq!(&bodies[&7].auxiliary as *const BodyAuxiliary, owner_address);
        assert_eq!(
            bodies[&7].auxiliary.properties.entry(0x1130).unwrap().value,
            u64::MAX
        );
        let data = bodies
            .get_mut(&7)
            .unwrap()
            .auxiliary
            .user_data
            .as_mut()
            .unwrap();
        assert_eq!(
            data.owner(),
            super::super::collidable_owner::UserData::Entity(71)
        );
        data.store_group_word(0xc, 0xabcdef12)?;
        data.store_group_word(0x10, 6)?;
        assert_eq!(
            (data.group_word(0xc)?, data.group_word(0x10)?),
            (0xabcdef12, 6)
        );
        assert_eq!(
            (data.entity, data.word_04, data.byte_08, data.byte_09),
            (Some(71), 0, 1, 0)
        );
        let before = *data;
        assert!(data.store_group_word(0x14, 1).is_err());
        assert_eq!(*data, before);
        assert_eq!(
            bodies[&7]
                .auxiliary
                .listeners
                .extended
                .as_ref()
                .unwrap()
                .entities
                .slots(),
            [Some(100)]
        );
        assert_eq!(bodies[&7].motion.current, original_motion);
        assert_eq!(bodies[&7].motion.quality, 8);
        assert!(bodies[&7].shapes.is_empty());
        assert_eq!(bodies.len(), 1);
        let mut backend = OwnedRuntime::new(&mut wrapper, &mut bodies, 100)?;
        assert!(super::super::character_body_info::RuntimeBackend::property(
            &mut backend,
            999,
            1,
            2
        )
        .is_err());
        Ok(())
    }
    #[test]
    fn owned_character_publication_changes_properties_listener_slots_and_wrapper() -> Result<()> {
        let mut properties = super::super::body_properties::Properties::default();
        properties.add(0x1130, 55)?;
        let mut bodies: BTreeMap<usize, _> = BTreeMap::from([(
            7,
            BodyAuxiliary {
                properties,
                listeners: Default::default(),
                user_data: None,
            },
        )]);
        let mut wrapper = [0xa5; 0x70];
        wrapper[0x10..0x14].copy_from_slice(&7u32.to_be_bytes());
        let mut source = [0; 0x70];
        source[0x44..0x48].copy_from_slice(&1f32.to_be_bytes());
        source[0x54..0x58].copy_from_slice(&2f32.to_be_bytes());
        source[0x68..0x6c].copy_from_slice(&0x80000000u32.to_be_bytes());
        for _ in 0..2 {
            let mut backend = OwnedRuntime::new(&mut wrapper, &mut bodies, 100)?;
            super::super::character_body_info::initialize_runtime(&source, [0; 2], &mut backend)?;
        }
        assert_eq!(bodies[&7].properties.entry(0x1130).unwrap().value, 55);
        assert_eq!(
            bodies[&7]
                .listeners
                .extended
                .as_ref()
                .unwrap()
                .entities
                .slots(),
            [Some(100), Some(100)]
        );
        assert_eq!(&wrapper[0x20..0x30], &source[0x40..0x50]);
        assert_eq!(&wrapper[0x34..0x38], &1f32.to_be_bytes());
        assert_eq!(&wrapper[0x60..0x64], &2f32.to_be_bytes());
        assert_eq!(wrapper[0x14..0x18], [0; 4]);
        assert_eq!(wrapper[0x50..0x60], [0; 16]);
        assert_eq!(wrapper[..0x10], [0xa5; 0x10]);
        assert_eq!(wrapper[0x64..], [0xa5; 12]);
        bodies.get_mut(&7).unwrap().properties.remove(0x1130);
        wrapper[0x14..0x18].fill(0xa5);
        wrapper[0x34..0x38].fill(0xa5);
        source[0x50..0x54].copy_from_slice(&1e20f32.to_be_bytes());
        assert!(super::super::character_body_info::initialize_runtime(
            &source,
            [0; 2],
            &mut OwnedRuntime::new(&mut wrapper, &mut bodies, 100)?
        )
        .is_err());
        assert_eq!(
            bodies[&7].properties.entry(0x1130).unwrap().value,
            0xffffffff80000000
        );
        assert_eq!(&wrapper[0x20..0x30], &source[0x40..0x50]);
        assert_eq!(wrapper[0x34..0x38], [0xa5; 4]);
        assert_eq!(wrapper[0x14..0x18], [0xa5; 4]);
        assert_eq!(
            bodies[&7]
                .listeners
                .extended
                .as_ref()
                .unwrap()
                .entities
                .count(),
            2
        );
        wrapper[0x10..0x14].copy_from_slice(&999u32.to_be_bytes());
        let prior = wrapper;
        assert!(super::super::character_body_info::initialize_runtime(
            &source,
            [0; 2],
            &mut OwnedRuntime::new(&mut wrapper, &mut bodies, 100)?
        )
        .is_err());
        assert_eq!(wrapper, prior);
        assert_eq!(bodies.len(), 1);
        Ok(())
    }
}
