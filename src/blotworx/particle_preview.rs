//! Explicit root-particle inspection. Its isolated Lua VM may run authored
//! material initialization; it is not the game's effect entity factory.
use super::{actor_scripts::Scripts, brush_assets::Assets, brush_particles::Hose};
use anyhow::{ensure, Context, Result};
pub struct Instance {
    pub name: String,
    pub particles: Hose,
    components: Vec<epic_mickey_lib_rs::scene_file::Component>,
    callbacks: Option<Callbacks>,
    deliveries: Vec<serde_json::Value>,
}
struct Callbacks {
    scripts: Scripts,
    outputs: super::actor_io::Outputs,
    time: f32,
}
impl Instance {
    pub fn load(
        name: &str,
        graph: &Assets,
        files: &[epic_mickey_lib_rs::packfile::VirtualFile],
    ) -> Result<Self> {
        let root = &graph.definitions[&graph.root];
        let model = root
            .model
            .as_ref()
            .context("Particle template root has no model")?;
        Ok(Self {
            name: name.into(),
            particles: Hose::load_attached(&graph.models[model], files)?,
            components: root.components.clone(),
            callbacks: None,
            deliveries: Vec::new(),
        })
    }
    pub fn initialize_materials(&mut self, world_scripts: &Scripts) -> Result<()> {
        ensure!(
            self.callbacks.is_none(),
            "Particle material inspection already initialized"
        );
        let scripts = world_scripts.resource_inspection(&self.name)?;
        let mut outputs = super::actor_io::Outputs::default();
        outputs.bind(0, &self.name, &self.components)?;
        outputs.emit(0, "OnSpawned", 0, 0.);
        self.callbacks = Some(Callbacks {
            scripts,
            outputs,
            time: 0.,
        });
        self.advance_callbacks(0.)
    }
    fn advance_callbacks(&mut self, delta: f32) -> Result<()> {
        let Some(runtime) = &mut self.callbacks else {
            return Ok(());
        };
        runtime.time += delta;
        ensure!(runtime.time.is_finite(), "Particle callback clock overflow");
        let time = runtime.time;
        let deliveries = runtime.outputs.ready(time);
        let mut execution = runtime.scripts.detach_scheduler()?;
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            execution.with_world_host(|lua,api,args| {
                if api!="SetR3MTConstRegOnSubGraph" {
                    return Err(mlua::Error::external(format!("Root-particle material inspection cannot execute {api}; use the full effect entity lifecycle")));
                }
                let request=super::subgraph_color::parse(lua,&runtime.scripts,args)?;
                if request.entity==Some(0) {
                    self.particles.set_subgraph_constant(request.name.as_deref(),request.slot,request.color).map_err(mlua::Error::external)?;
                }
                Ok(mlua::MultiValue::new())
            },|scripts| {
                ensure!(scripts.advance(time)?.is_empty(),"Particle material callback emitted gameplay commands");
                for delivery in deliveries {
                    let connection=delivery.connection;
                    ensure!(connection.target=="_self" || connection.target=="_activator" || connection.target==self.name,
                        "Particle material inspection needs a live world for target {}",connection.target);
                    if !scripts.has_callback(&connection.input)? {
                        // Catalog entries are provider hints only. Execute the actual
                        // imported chunk and verify that it defines the callback.
                        let symbol=crate::game_api::catalog().symbols.iter().find(|s|s.name==connection.input)
                            .with_context(||format!("No callback provider hint for {}",connection.input))?;
                        let paths=symbol.definitions.iter().map(|d|d.path.as_str()).collect::<std::collections::BTreeSet<_>>();
                        ensure!(paths.len()==1,"Ambiguous callback provider for {}",connection.input);
                        scripts.ensure_resource_module(paths.first().unwrap())?;
                    }
                    ensure!(scripts.has_callback(&connection.input)?,"Imported module did not define {}",connection.input);
                    scripts.call_io(&connection.input,super::actor_scripts::CallbackContext{entity:Some(0),activator:Some(0)},&connection.parameters)?;
                    self.deliveries.push(serde_json::json!({"input":connection.input,"parameters":connection.parameters,"time":time}));
                }Ok(())
            })
        }));
        runtime.scripts.restore_scheduler(execution)?;
        match outcome {
            Ok(result) => result,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }
    pub fn advance_attached(&mut self, dt: f32, enabled: bool, owner: [f32; 16]) -> Result<()> {
        ensure!(
            dt.is_finite() && dt >= 0.,
            "Invalid particle inspection delta"
        );
        self.advance_callbacks(dt)?;
        self.particles.advance_attached(dt, enabled, owner)
    }
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"template":self.name,"material_callbacks":self.deliveries,
            "colors":self.particles.color_summary(),"full_effect_entity":false,
            "pending_material_tasks":self.callbacks.as_ref().map(|runtime|runtime.scripts.pending_tasks().ok())})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_spawn_material_callback_reaches_particle_draws_and_world_models() -> Result<()> {
        let root = std::path::PathBuf::from(
            std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?,
        );
        let mut world = super::super::assets::load_with_particle_preview(
            &root,
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
            None,
            Some("gremlinvariant4teleportinfx"),
        )?;
        let preview = world.particle_asset_preview.as_mut().unwrap();
        assert_eq!(preview.particles.color_summary(), serde_json::json!([]));
        preview.initialize_materials(&world.scripts)?;
        let summary = preview.summary();
        assert_eq!(summary["material_callbacks"].as_array().unwrap().len(), 1);
        assert_eq!(
            summary["material_callbacks"][0]["input"],
            "SetupGremlinTeleportInFXColors"
        );
        assert_eq!(summary["material_callbacks"][0]["parameters"], "Variant4");
        let colors = summary["colors"].as_array().unwrap();
        assert_eq!(colors.len(), 2);
        for color in colors {
            assert_eq!(
                color["constant_colors"][1],
                serde_json::json!([85, 119, 78, 0])
            );
        }
        let owner = [
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
        ];
        let mut drawn = false;
        for _ in 0..45 {
            preview.advance_attached(1. / 30., true, owner)?;
            let mut draws = Vec::new();
            preview
                .particles
                .append([1., 0., 0.], [0., 1., 0.], &mut draws)?;
            for draw in draws {
                let program = draw.preview.as_ref().unwrap().as_ref().unwrap();
                assert_eq!(
                    draw.material_registers.unwrap()[4],
                    [85., 119., 78., 0.].map(|v| v / 255.)
                );
                assert_ne!(
                    program.register_defaults.unwrap()[4],
                    draw.material_registers.unwrap()[4],
                    "Publication mutated shared shader defaults"
                );
                drawn = true;
            }
        }
        assert!(drawn);
        assert!(preview.initialize_materials(&world.scripts).is_err());
        // The same API also reaches ordinary, already initialized World models.
        let player = world.player_entity;
        let hierarchy = world.entity_bound_hierarchy(player)?.unwrap();
        let node = hierarchy
            .nodes
            .values()
            .find(|node| node.authored_bound.is_some() && !node.name.is_empty())
            .context("Missing Mickey render node")?;
        let name = serde_json::to_string(&node.name)?;
        world.run_callback_fixture(&format!("ColorSlot=1.8; assert(select('#',SetR3MTConstRegOnSubGraph(GetPlayer(),{name},'ColorSlot',vector4(.337,.467,.306,0)))==0)"))?;
        assert!(world
            .render_objects
            .iter()
            .any(|(&(owner, _), state)| owner == player
                && state.constant_colors[1] == Some([85, 119, 78, 0])));
        let before = serde_json::to_value(
            world
                .render_objects
                .iter()
                .filter(|((owner, _), _)| *owner == player)
                .map(|(key, value)| serde_json::json!({"node":key.1,"state":value}))
                .collect::<Vec<_>>(),
        )?;
        world.run_callback_fixture("SetR3MTConstRegOnSubGraph(GetPlayer(),'BwxMissingSubgraph',1,vector4(1,0,0,0)); SetR3MTConstRegOnSubGraph(GetPlayer(),nil,-1,vector4(0/0,0,0,0)); SetR3MTConstRegOnSubGraph('BwxMissingOwner',nil,1,vector4(1,0,0,0))")?;
        let after = serde_json::to_value(
            world
                .render_objects
                .iter()
                .filter(|((owner, _), _)| *owner == player)
                .map(|(key, value)| serde_json::json!({"node":key.1,"state":value}))
                .collect::<Vec<_>>(),
        )?;
        assert_eq!(before, after);
        Ok(())
    }
}
