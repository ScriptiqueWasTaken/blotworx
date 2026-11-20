//! Concrete typed storage/conversion binding. Required external hierarchy and
//! related-record effects remain caller supplied, never defaulted to no-ops.
use super::{
    native_matrix::Matrix,
    native_transform_record::Transform,
    transform_dirty, transform_local, transform_rotation_scale,
    transform_storage::{Space, Storage},
    transform_translation, transform_world,
};
use anyhow::Result;
pub trait Hooks {
    fn children(&mut self, storage: &mut Storage<'_>) -> Result<()>;
    fn related(&mut self, storage: &mut Storage<'_>, offset: usize) -> Result<Option<usize>>;
    fn mark(
        &mut self,
        storage: &mut Storage<'_>,
        record: usize,
        offset: usize,
        value: u8,
    ) -> Result<()>;
}
pub struct Constants {
    pub zero: f32,
    pub one: f32,
    pub identity: Matrix,
}
pub struct Runtime<'a, H: Hooks> {
    pub storage: Storage<'a>,
    pub hooks: H,
    pub constants: Constants,
}
impl<H: Hooks> transform_dirty::Backend for Runtime<'_, H> {
    fn mark_children(&mut self) -> Result<()> {
        self.hooks.children(&mut self.storage)
    }
    fn related(&mut self, o: usize) -> Result<Option<usize>> {
        self.hooks.related(&mut self.storage, o)
    }
    fn mark(&mut self, r: usize, o: usize, v: u8) -> Result<()> {
        self.hooks.mark(&mut self.storage, r, o, v)
    }
}
impl<H: Hooks> transform_translation::Backend for Runtime<'_, H> {
    fn translation(&mut self) -> Result<[f32; 3]> {
        Ok(self.storage.position(Space::World))
    }
    fn store(&mut self, v: [f32; 3]) -> Result<()> {
        self.storage.store_position(Space::World, v);
        Ok(())
    }
    fn flags_20(&mut self) -> Result<u8> {
        self.storage.byte(0x20)
    }
    fn update(&mut self, _: u32) -> Result<()> {
        transform_local::position(self)
    }
}
impl<H: Hooks> transform_rotation_scale::Backend for Runtime<'_, H> {
    fn rotation(&mut self) -> Result<Matrix> {
        Ok(self.storage.rotation(Space::World))
    }
    fn store_rotation(&mut self, v: Matrix) -> Result<()> {
        self.storage.store_rotation(Space::World, v);
        Ok(())
    }
    fn scale(&mut self) -> Result<f32> {
        Ok(self.storage.world_scale())
    }
    fn store_scale(&mut self, v: f32) -> Result<()> {
        self.storage.store_world_scale(v);
        Ok(())
    }
    fn flags_20(&mut self) -> Result<u8> {
        self.storage.byte(0x20)
    }
    fn local_rotation_354638(&mut self, _: u32) -> Result<()> {
        transform_local::rotation(self)
    }
}
impl<H: Hooks> transform_local::Backend for Runtime<'_, H> {
    fn world_position(&mut self) -> Result<[f32; 3]> {
        Ok(self.storage.position(Space::World))
    }
    fn world_rotation(&mut self) -> Result<Matrix> {
        Ok(self.storage.rotation(Space::World))
    }
    fn cached_parent_e8(&mut self) -> Result<Transform> {
        Ok(self.storage.cached_parent())
    }
    fn byte(&mut self, o: usize) -> Result<u8> {
        self.storage.byte(o)
    }
    fn zero_4cd728(&mut self) -> Result<f32> {
        Ok(self.constants.zero)
    }
    fn identity_80767b64(&mut self) -> Result<Matrix> {
        Ok(self.constants.identity)
    }
    fn set_local_position_351e10(&mut self, v: [f32; 3]) -> Result<()> {
        if self.storage.position(Space::Local) != v {
            self.storage.store_position(Space::Local, v);
        }
        Ok(())
    }
    fn set_local_rotation_351ef0(&mut self, v: Matrix) -> Result<()> {
        if self.storage.rotation(Space::Local) != v {
            self.storage.store_rotation(Space::Local, v);
        }
        Ok(())
    }
}
impl<H: Hooks> transform_world::Backend for Runtime<'_, H> {
    fn byte(&mut self, o: usize) -> Result<u8> {
        self.storage.byte(o)
    }
    fn local_position(&mut self) -> Result<[f32; 3]> {
        Ok(self.storage.position(Space::Local))
    }
    fn local_rotation(&mut self) -> Result<Matrix> {
        Ok(self.storage.rotation(Space::Local))
    }
    fn zero_4cd728(&mut self) -> Result<f32> {
        Ok(self.constants.zero)
    }
    fn identity_80767b64(&mut self) -> Result<Matrix> {
        Ok(self.constants.identity)
    }
    fn one_4cd748(&mut self) -> Result<f32> {
        Ok(self.constants.one)
    }
    fn cached_parent_e8(&mut self) -> Result<Transform> {
        Ok(self.storage.cached_parent())
    }
    fn set_world_position_351c10(&mut self, v: [f32; 3]) -> Result<()> {
        transform_translation::set_inner(v, self)?;
        Ok(())
    }
    fn set_world_rotation_351d28(&mut self, v: Matrix) -> Result<()> {
        transform_rotation_scale::rotation_inner(v, self)?;
        Ok(())
    }
    fn set_world_scale_351b28(&mut self, v: f32) -> Result<()> {
        transform_rotation_scale::scale(v, self)?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Effects {
        children: usize,
        marks: Vec<(usize, usize, u8)>,
        fail: bool,
    }
    impl Hooks for Effects {
        fn children(&mut self, _: &mut Storage<'_>) -> Result<()> {
            self.children += 1;
            if self.fail {
                anyhow::bail!("Hierarchy unavailable")
            };
            Ok(())
        }
        fn related(&mut self, _: &mut Storage<'_>, o: usize) -> Result<Option<usize>> {
            Ok(Some(o))
        }
        fn mark(&mut self, _: &mut Storage<'_>, r: usize, o: usize, v: u8) -> Result<()> {
            self.marks.push((r, o, v));
            Ok(())
        }
    }
    #[test]
    fn owned_conversion_inner_publication_and_failure_share_storage() -> Result<()> {
        let id = [1., 0., 0., 0., 1., 0., 0., 0., 1.];
        let mut bytes = [0; 0x140];
        let mut storage = Storage::new(&mut bytes)?;
        storage.store_position(Space::Local, [-5., 4., 6.]);
        storage.store_rotation(Space::Local, id);
        storage.store_cached_parent([0., 1., 0., -1., 0., 0., 0., 0., 1., 2., 4., 6., 2.]);
        let mut r = Runtime {
            storage,
            hooks: Effects {
                children: 0,
                marks: vec![],
                fail: false,
            },
            constants: Constants {
                zero: 0.,
                one: 1.,
                identity: id,
            },
        };
        transform_world::update(&mut r)?;
        assert_eq!(r.storage.position(Space::World), [10., 14., 18.]);
        assert_eq!(r.storage.position(Space::Local), [-5., 4., 6.]);
        assert_eq!(r.hooks.children, 2);
        assert_eq!(r.hooks.marks.len(), 4);
        assert!(transform_translation::set([2., 4., 6.], &mut r)?);
        assert_eq!(r.storage.position(Space::Local), [0.; 3]);
        assert_eq!(r.hooks.children, 3);
        r.hooks.fail = true;
        assert!(transform_translation::set([10., 14., 18.], &mut r).is_err());
        assert_eq!(r.storage.position(Space::World), [10., 14., 18.]);
        assert_eq!(r.storage.position(Space::Local), [0.; 3]);
        Ok(())
    }
}
