//! Original pose relocation REL29866c..2987e8. Backends must implement actual
//! component effects; this protocol alone is not the Lua MoveToEntity binding.
use anyhow::Result;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Component {
    Transform,
    BasicAi,
    RigidBody,
    SceneGraph,
    Lod,
    Controller,
    Spline,
    Phantom,
}
/// Resolved Lua callback26b1dc request. Native argument3 is probe and argument4
/// is reset; the lower-level298568 call receives reset before probe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub entity: usize,
    pub target: usize,
    pub reset: bool,
    pub probe: bool,
}
impl Request {
    pub fn execute(self, backend: &mut impl EntityBackend) -> Result<()> {
        to_entity(
            self.entity,
            self.target,
            self.reset as u32,
            self.probe as u32,
            backend,
        )
    }
}
pub trait Backend {
    fn component(&mut self, entity: usize, kind: Component) -> Result<Option<usize>>;
    fn detach_parent(&mut self, transform: usize) -> Result<()>;
    fn set_position(&mut self, entity: usize, position: [f32; 4]) -> Result<()>;
    fn set_rotation(&mut self, entity: usize, rotation: [f32; 12]) -> Result<()>;
    fn update_children(&mut self, transform: usize) -> Result<()>;
    fn ai_facing(&mut self, component: usize, forward: [f32; 3]) -> Result<()>;
    fn rigid_body_pose(&mut self, component: usize, entity: usize) -> Result<()>;
    fn owner_time(&mut self, scene_graph: usize) -> Result<f32>;
    fn scene_graph_update(&mut self, component: usize, time: f32, flag: u32) -> Result<()>;
    fn reset_lod(&mut self, component: usize) -> Result<()>;
    fn reset_controller(&mut self, component: usize, flag: u32) -> Result<()>;
    fn rebuild_spline(&mut self, component: usize) -> Result<()>;
    fn phantom_position(&mut self, component: usize, position: [f32; 4]) -> Result<()>;
}
pub trait EntityBackend: Backend {
    fn entity_position(&mut self, entity: usize) -> Result<[f32; 4]>;
    fn entity_rotation(&mut self, entity: usize) -> Result<[f32; 12]>;
    /// Controller helper305660; actual ground query/state is backend-owned.
    fn ground_probe(
        &mut self,
        controller: usize,
        position: &mut [f32; 4],
        distance: f32,
    ) -> Result<f32>;
}
/// REL298568 reads source position before rotation;298874 optionally probes
/// the destination controller, then calls the ordered relocation sequence.
pub fn to_entity(
    entity: usize,
    target: usize,
    reset_flag: u32,
    probe_flag: u32,
    backend: &mut impl EntityBackend,
) -> Result<()> {
    let mut position = backend.entity_position(target)?;
    let rotation = backend.entity_rotation(target)?;
    if probe_flag != 0 {
        if let Some(controller) = backend.component(entity, Component::Controller)? {
            let distance = backend.ground_probe(controller, &mut position, 4.)?;
            if distance < 4. {
                position[1] -= distance;
                // This setter precedes the main sequence's parent detachment.
                backend.set_position(entity, position)?;
            }
        }
    }
    pose(entity, position, rotation, reset_flag, backend)
}
/// Queries are interleaved with updates; callbacks may change later membership.
/// Native failures are not transactional. A backend error leaves earlier effects
/// published and stops; no rollback or automatic retry is fabricated.
pub fn pose(
    entity: usize,
    position: [f32; 4],
    rotation: [f32; 12],
    reset_flag: u32,
    backend: &mut impl Backend,
) -> Result<()> {
    let transform = backend.component(entity, Component::Transform)?;
    if let Some(component) = transform {
        backend.detach_parent(component)?;
    }
    backend.set_position(entity, position)?;
    backend.set_rotation(entity, rotation)?;
    if let Some(component) = transform {
        backend.update_children(component)?;
    }
    if let Some(component) = backend.component(entity, Component::BasicAi)? {
        backend.ai_facing(component, [rotation[8], rotation[9], rotation[10]])?;
    }
    if let Some(component) = backend.component(entity, Component::RigidBody)? {
        backend.rigid_body_pose(component, entity)?;
    }
    if let Some(component) = backend.component(entity, Component::SceneGraph)? {
        let time = backend.owner_time(component)?;
        backend.scene_graph_update(component, time, 1)?;
    }
    if let Some(component) = backend.component(entity, Component::Lod)? {
        backend.reset_lod(component)?;
    }
    // Native still queries the controller when reset_flag is zero.
    if let Some(component) = backend.component(entity, Component::Controller)? {
        if reset_flag != 0 {
            backend.reset_controller(component, 0)?;
        }
    }
    if let Some(component) = backend.component(entity, Component::Spline)? {
        backend.rebuild_spline(component)?;
    }
    if let Some(component) = backend.component(entity, Component::Phantom)? {
        backend.phantom_position(component, position)?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Trace {
        calls: Vec<String>,
        fail: bool,
        probe: f32,
        expected_y: f32,
    }
    impl Backend for Trace {
        fn component(&mut self, e: usize, k: Component) -> Result<Option<usize>> {
            assert_eq!(e, 7);
            self.calls.push(format!("query {k:?}"));
            Ok(Some(8))
        }
        fn detach_parent(&mut self, _: usize) -> Result<()> {
            self.calls.push("detach".into());
            Ok(())
        }
        fn set_position(&mut self, _: usize, p: [f32; 4]) -> Result<()> {
            assert_eq!(p, [1., self.expected_y, 3., 4.]);
            self.calls.push("position".into());
            Ok(())
        }
        fn set_rotation(&mut self, _: usize, r: [f32; 12]) -> Result<()> {
            assert_eq!(r[8..12], [8., 9., 10., 11.]);
            self.calls.push("rotation".into());
            Ok(())
        }
        fn update_children(&mut self, _: usize) -> Result<()> {
            self.calls.push("children".into());
            Ok(())
        }
        fn ai_facing(&mut self, _: usize, f: [f32; 3]) -> Result<()> {
            assert_eq!(f, [8., 9., 10.]);
            self.calls.push("ai".into());
            Ok(())
        }
        fn rigid_body_pose(&mut self, _: usize, e: usize) -> Result<()> {
            assert_eq!(e, 7);
            self.calls.push("body".into());
            if self.fail {
                anyhow::bail!("Body unavailable")
            };
            Ok(())
        }
        fn owner_time(&mut self, _: usize) -> Result<f32> {
            self.calls.push("time".into());
            Ok(3.5)
        }
        fn scene_graph_update(&mut self, _: usize, t: f32, f: u32) -> Result<()> {
            assert_eq!((t, f), (3.5, 1));
            self.calls.push("graph".into());
            Ok(())
        }
        fn reset_lod(&mut self, _: usize) -> Result<()> {
            self.calls.push("lod".into());
            Ok(())
        }
        fn reset_controller(&mut self, _: usize, f: u32) -> Result<()> {
            assert_eq!(f, 0);
            self.calls.push("controller".into());
            Ok(())
        }
        fn rebuild_spline(&mut self, _: usize) -> Result<()> {
            self.calls.push("spline".into());
            Ok(())
        }
        fn phantom_position(&mut self, _: usize, p: [f32; 4]) -> Result<()> {
            assert_eq!(p, [1., self.expected_y, 3., 4.]);
            self.calls.push("phantom".into());
            Ok(())
        }
    }
    impl EntityBackend for Trace {
        fn entity_position(&mut self, e: usize) -> Result<[f32; 4]> {
            assert_eq!(e, 9);
            self.calls.push("source position".into());
            Ok([1., 2., 3., 4.])
        }
        fn entity_rotation(&mut self, e: usize) -> Result<[f32; 12]> {
            assert_eq!(e, 9);
            self.calls.push("source rotation".into());
            Ok(std::array::from_fn(|i| i as f32))
        }
        fn ground_probe(&mut self, c: usize, p: &mut [f32; 4], d: f32) -> Result<f32> {
            assert_eq!(c, 8);
            assert_eq!(*p, [1., 2., 3., 4.]);
            assert_eq!(d, 4.);
            p[1] += 0.25;
            self.calls.push("probe".into());
            Ok(self.probe)
        }
    }
    #[test]
    fn entity_move_probes_before_detachment_and_preserves_native_threshold() -> Result<()> {
        for distance in [-1., 0., 3.5, 4., 5., f32::NAN] {
            let expected_y = if distance < 4. { 2.25 - distance } else { 2.25 };
            let mut trace = Trace {
                calls: vec![],
                fail: false,
                probe: distance,
                expected_y,
            };
            to_entity(7, 9, 0, 2, &mut trace)?;
            assert_eq!(
                &trace.calls[..4],
                [
                    "source position",
                    "source rotation",
                    "query Controller",
                    "probe"
                ]
            );
            let next = if distance < 4. {
                assert_eq!(trace.calls[4], "position");
                5
            } else {
                4
            };
            assert_eq!(
                &trace.calls[next..next + 4],
                ["query Transform", "detach", "position", "rotation"]
            );
        }
        let mut trace = Trace {
            calls: vec![],
            fail: false,
            probe: -1.,
            expected_y: 2.,
        };
        to_entity(7, 9, 0, 0, &mut trace)?;
        assert!(!trace.calls.iter().any(|c| c == "probe"));
        assert_eq!(
            &trace.calls[..3],
            ["source position", "source rotation", "query Transform"]
        );
        Ok(())
    }
    #[test]
    fn native_relocation_preserves_query_and_effect_order() -> Result<()> {
        let mut trace = Trace {
            calls: vec![],
            fail: false,
            probe: 4.,
            expected_y: 2.,
        };
        pose(
            7,
            [1., 2., 3., 4.],
            std::array::from_fn(|i| i as f32),
            2,
            &mut trace,
        )?;
        assert_eq!(
            trace.calls,
            vec![
                "query Transform",
                "detach",
                "position",
                "rotation",
                "children",
                "query BasicAi",
                "ai",
                "query RigidBody",
                "body",
                "query SceneGraph",
                "time",
                "graph",
                "query Lod",
                "lod",
                "query Controller",
                "controller",
                "query Spline",
                "spline",
                "query Phantom",
                "phantom"
            ]
        );
        trace.calls.clear();
        pose(
            7,
            [1., 2., 3., 4.],
            std::array::from_fn(|i| i as f32),
            0,
            &mut trace,
        )?;
        assert!(trace.calls.iter().any(|c| c == "query Controller"));
        assert!(!trace.calls.iter().any(|c| c == "controller"));
        trace.calls.clear();
        trace.fail = true;
        assert!(pose(
            7,
            [1., 2., 3., 4.],
            std::array::from_fn(|i| i as f32),
            1,
            &mut trace
        )
        .is_err());
        assert_eq!(trace.calls.last().unwrap(), "body");
        assert!(!trace.calls.iter().any(|c| c == "query SceneGraph"));
        Ok(())
    }
}
