//! Native AudioController ownership shared by Lua, authored IO and updates.
use super::super::multi_position_audio::{self as native, Host};
use super::*;

struct AudioHost<'a> {
    world: &'a mut World,
    owner: usize,
    component: u32,
    sources: &'a [Option<usize>],
}
impl AudioHost<'_> {
    fn component(&self) -> Result<&epic_mickey_lib_rs::scene_file::Component> {
        self.world
            .entities
            .get(self.owner)
            .context("Audio owner missing")?
            .components
            .iter()
            .find(|component| component.link_id.to_u32() == self.component)
            .context("Audio component missing")
    }
}
impl Host for AudioHost<'_> {
    fn event(&mut self) -> Result<String> {
        Ok(native::property(self.component()?, "Audio Start Event")?
            .as_str()
            .context("Invalid audio start event")?
            .to_owned())
    }
    fn post(&mut self, event: &str) -> Result<u32> {
        let id = self
            .world
            .audio_playback
            .post_spatial_from_game(event, self.owner);
        let limitation="Multi-position controller source/lifecycle requests are connected; spatial object registration, bank/container playback, buses and mixing remain unimplemented. Posts return invalid playing ID0 and remain in rejected_posts; no audible playback is claimed.";
        if !self
            .world
            .report
            .unsupported
            .iter()
            .any(|line| line == limitation)
        {
            self.world.report.unsupported.push(limitation.into());
        }
        Ok(id)
    }
    fn positions(&mut self) -> Result<Vec<[f32; 4]>> {
        let condition = i32::try_from(
            native::property(self.component()?, "Play Condition")?
                .as_i64()
                .context("Invalid audio play condition")?,
        )?;
        let mut positions = Vec::with_capacity(self.sources.len());
        for &source in self.sources {
            let Some(source) = source else {
                continue;
            };
            if self.world.removed_entities.contains(&source) {
                continue;
            }
            let runtime = self
                .world
                .entities
                .get(source)
                .context("Cached audio source missing")?;
            let painted = if condition == 0 {
                None
            } else {
                runtime
                    .components
                    .iter()
                    .find(|component| {
                        matches!(
                            component.class_name.as_str(),
                            "PhysicsJigsawComponent"
                                | "PhysicsRigidBodyCharacterComponent"
                                | "PhysicsRigidBodyDynamicComponent"
                                | "PhysicsRigidBodyPhantomShapeComponent"
                                | "PhysicsRigidBodyStaticComponent"
                        )
                    })
                    .map(|component| {
                        component
                            .properties
                            .iter()
                            .find(|p| p.name == "Is Painted")
                            .and_then(|p| p.value.as_bool())
                            .context("Audio source has no resolved physics paint flag")
                    })
                    .transpose()?
            };
            if !native::admits(condition, painted) {
                continue;
            }
            if let Some([x, y, z]) = self.world.entity_position_or_bound_center(source)? {
                positions.push([x, y, z, 0.]);
            }
        }
        Ok(positions)
    }
    fn publish_positions(&mut self, _positions: &[[f32; 4]]) -> Result<()> {
        // Wwise's result is ignored by2fecec. Record the actual controller
        // inputs in its single owned positions vector, not a duplicate trace.
        // This counts an attempt, never an accepted/audible backend submission.
        self.world.audio_playback.spatial_position_requests += 1;
        Ok(())
    }
    fn stop(&mut self, _playing_id: u32) -> Result<()> {
        anyhow::bail!(
            "Native StopPlayingID command queue and audio-device clock execution are not connected"
        )
    }
}
impl World {
    pub(super) fn initialize_multi_position_audio(&mut self, range: std::ops::Range<usize>) {
        for owner in range {
            for component in self.entities[owner]
                .components
                .iter()
                .filter(|c| c.class_name == "AudioControllerComponent")
            {
                let scene = &self.entities[owner].scene;
                let controller = native::Controller::from_component(component, |id| {
                    self.entities
                        .iter()
                        .enumerate()
                        .find(|(index, entity)| {
                            entity.id == id
                                && entity.scene == *scene
                                && !self.removed_entities.contains(index)
                        })
                        .map(|(index, _)| index)
                });
                match controller {
                    Ok(controller) => {
                        self.multi_position_audio
                            .insert((owner, controller.component), controller);
                    }
                    Err(error) => self.report.unsupported.push(format!(
                        "Audio controller {}: {error:#}",
                        self.entities[owner].name
                    )),
                }
            }
        }
    }
    pub(super) fn control_multi_position_audio(&mut self, owner: usize, start: bool) -> Result<()> {
        if self.removed_entities.contains(&owner) {
            return Ok(());
        }
        let Some(component) = self
            .entities
            .get(owner)
            .context("Audio target missing")?
            .components
            .iter()
            .find(|component| component.class_name == "AudioControllerComponent")
        else {
            return Ok(());
        };
        let key = (owner, component.link_id.to_u32());
        self.run_multi_position_audio(key, Some(start))
    }
    fn run_multi_position_audio(&mut self, key: (usize, u32), command: Option<bool>) -> Result<()> {
        let mut controller = self
            .multi_position_audio
            .remove(&key)
            .context("Audio controller initialization failed")?;
        // Move the cached handle array rather than cloning it every update.
        let sources = std::mem::take(&mut controller.sources);
        let mut host = AudioHost {
            world: self,
            owner: key.0,
            component: key.1,
            sources: &sources,
        };
        let result = match command {
            Some(true) => controller.start(&mut host),
            Some(false) => controller.stop(&mut host),
            None => controller.update(&mut host),
        };
        controller.sources = sources;
        controller.error = result.as_ref().err().map(|error| format!("{error:#}"));
        self.multi_position_audio.insert(key, controller);
        result
    }
    pub(super) fn advance_multi_position_audio(&mut self) {
        let keys = self
            .multi_position_audio
            .iter()
            .filter(|((owner, _), controller)| {
                controller.active
                    && controller.dynamic
                    && !self.removed_entities.contains(owner)
                    && self.entity_scene_active(*owner)
            })
            .map(|(&key, _)| key)
            .collect::<Vec<_>>();
        for key in keys {
            if let Err(error) = self.run_multi_position_audio(key, None) {
                let message = format!(
                    "Audio source update {}: {error:#}",
                    self.entities[key.0].name
                );
                if !self.report.unsupported.contains(&message) {
                    self.report.unsupported.push(message);
                }
            }
        }
    }
}
