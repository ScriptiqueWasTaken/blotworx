//! Retail mesh traversal, REL 0x258698. The scene adapter must supply live
//! world bounds and controller classification, not flattened draw-call bounds.
use anyhow::Result;

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Mesh,
    Node,
    Other,
}
pub struct Object<I> {
    pub kind: Kind,
    /// DecalType integer extra data; absent or wrong extra-data type is -1.
    pub decal_type: i32,
    pub center: [f32; 3],
    pub radius: f32,
    pub flags: u16,
    /// Controller-chain RTTI matches 0x807184e0 or 0x80717e68.
    pub separate_controller: bool,
    pub children: Vec<Option<I>>,
}
pub trait Scene {
    type Id: Copy;
    fn object(&self, id: Self::Id) -> Result<Object<Self::Id>>;
}

pub fn collect<S: Scene>(
    scene: &S,
    root: S::Id,
    position: [f32; 3],
    radius: f32,
    radius_squared: f32,
    meshes: &mut Vec<S::Id>,
    separate: &mut impl FnMut(S::Id) -> bool,
) -> Result<()> {
    let object = scene.object(root)?;
    if object.decal_type == 0 {
        return Ok(());
    }
    match object.kind {
        Kind::Mesh => {
            meshes.push(root);
            return Ok(());
        }
        Kind::Other => return Ok(()),
        Kind::Node => {}
    }
    for child in object.children.into_iter().flatten() {
        let object = scene.object(child)?;
        if object.decal_type == 0 {
            continue;
        }
        let [x, y, z] = std::array::from_fn(|i| object.center[i] - position[i]);
        // Preserve paired-single operation order, including fused operations.
        let distance = z.mul_add(z, x * x) + y * y;
        let threshold = object.radius.mul_add(
            object.radius,
            (2. * object.radius).mul_add(radius, radius_squared),
        );
        if distance >= threshold {
            continue;
        }
        if object.decal_type == 1 || object.flags & 0x200 != 0 || object.separate_controller {
            // A full queue drops this child; it does not fall back to projecting
            // it together with its parent. Original check is 0x2589bc.
            separate(child);
        } else if object.kind == Kind::Mesh {
            meshes.push(child);
        } else {
            collect(
                scene,
                child,
                position,
                radius,
                radius_squared,
                meshes,
                separate,
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Tree;
    impl Scene for Tree {
        type Id = u32;
        fn object(&self, id: u32) -> Result<Object<u32>> {
            Ok(Object {
                kind: if id == 0 { Kind::Node } else { Kind::Mesh },
                decal_type: match id {
                    1 => 0,
                    2 => 1,
                    _ => -1,
                },
                center: if id == 5 { [2., 0., 0.] } else { [0.; 3] },
                radius: 1.,
                flags: if id == 3 { 0x200 } else { 0 },
                separate_controller: id == 4,
                children: if id == 0 {
                    vec![Some(1), None, Some(2), Some(3), Some(4), Some(5), Some(6)]
                } else {
                    vec![]
                },
            })
        }
    }
    #[test]
    fn authored_exclusions_bounds_and_separate_children_preserve_order() -> Result<()> {
        let mut meshes = vec![];
        let mut children = vec![];
        collect(&Tree, 0, [0.; 3], 1., 1., &mut meshes, &mut |id| {
            children.push(id);
            false
        })?;
        assert_eq!(children, [2, 3, 4]);
        assert_eq!(meshes, [6]); // Child 5 touches the boundary and is rejected.
                                 // Root meshes bypass child bounds and separate-projection classification.
        collect(&Tree, 2, [100.; 3], 1., 1., &mut meshes, &mut |_| {
            panic!("root deferred")
        })?;
        assert_eq!(meshes, [6, 2]);
        Ok(())
    }
}
