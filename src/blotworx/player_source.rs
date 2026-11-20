//! Source-based player admission, independent of archive byte order/game title.
use crate::render_support::scene;
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::scene_file::{Entity, SceneFile};
/// PC PlayerManager::SpawnEntity (977e10..977eb1). Control types zero
/// and three both use the human Oswald archetype; Wii U's table differs.
pub fn pc_archetype(player:u32,control:i32)->Option<&'static str> {
    match (player,control) {
        (0,0..=3)=>Some("Player"),
        (1,0|3)=>Some("PlayerTwo"),
        (1,1)=>Some("AI_Oswald_2"),
        (1,2)=>Some("AI_Oswald_2D"),
        _=>None,
    }
}
pub struct Source {
    pub scene: SceneFile,
    pub player: usize,
    pub marker: String,
    pub player_index: u32,
}
pub fn prepare(
    scenes: &[SceneFile],
    palette: &SceneFile,
    prototype: &str,
    player_index: u32,
) -> Result<Source> {
    let mut starts = Vec::new();
    for source in scenes {
        for entity in &source.objects {
            if !source.scene.contains(&entity.link_id) {
                continue;
            }
            let components = scene::get_effective_components(entity, &source.objects);
            for component in components
                .iter()
                .filter(|c| c.class_name == "PlayerStartComponent")
            {
                let index = component
                    .properties
                    .iter()
                    .find(|p| p.name == "PlayerIndex")
                    .map(|p| p.value.as_u64().context("Invalid PlayerIndex"))
                    .transpose()?
                    .unwrap_or(0);
                if index != u64::from(player_index) {
                    continue;
                }
                let transform_component = components
                    .iter()
                    .find(|c| c.class_name == "JPSTransformationComponent")
                    .context("Player start has no transform")?;
                for prop in &transform_component.properties {
                    if prop.name == "Parent Entity" {
                        ensure!(
                            prop.value.as_u64().unwrap_or(0) == 0,
                            "Parented player-start transform requires world composition"
                        );
                    }
                    if prop.name == "Parent Entity Name" {
                        ensure!(
                            prop.value.as_str().unwrap_or("").is_empty(),
                            "Named-parent player start requires world composition"
                        );
                    }
                }
                starts.push((
                    entity.name.clone(),
                    scene::get_entity_transform(entity, &source.objects),
                ));
            }
        }
    }
    ensure!(
        starts.len() == 1,
        "Expected one active player start for index {player_index}, found {}",
        starts.len()
    );
    let (marker, transform) = starts.pop().unwrap();
    ensure!(
        transform
            .translation
            .iter()
            .chain(transform.rotation.iter().flatten())
            .chain(transform.scale.iter())
            .all(|v| v.is_finite()),
        "Non-finite player-start transform"
    );
    ensure!(
        transform.scale[0] == transform.scale[1] && transform.scale[0] == transform.scale[2],
        "Player-start scale is nonuniform"
    );
    let candidates = palette
        .objects
        .iter()
        .enumerate()
        .filter(|(_, e)| e.name.eq_ignore_ascii_case(prototype))
        .collect::<Vec<_>>();
    ensure!(
        candidates.len() == 1,
        "Missing or ambiguous dynamic player archetype {prototype}"
    );
    let mut instance = crate::scene_authoring::instantiate(&palette.objects, candidates[0].0)?;
    scene::set_entity_transform(&mut instance, transform);
    let mut source = SceneFile::new(
        palette.objects.clone(),
        vec![instance.link_id],
        palette.em2_extra_strings.clone(),
        palette.unique_id,
        palette.version.clone(),
    );
    let player = source.objects.len();
    source.objects.push(instance);
    Ok(Source {
        scene: source,
        player,
        marker,
        player_index,
    })
}
impl Source {
    pub fn entity(&self) -> &Entity {
        &self.scene.objects[self.player]
    }
    /// Admit the original character shape, independently of model/animation data.
    pub fn collision_hull(
        &self,
        files: &[epic_mickey_lib_rs::packfile::VirtualFile],
    ) -> Result<(
        String,
        super::character_hull::ConvexHull,
        super::native_gsk::ConvexSupport,
    )> {
        let components = scene::get_effective_components(self.entity(), &self.scene.objects);
        let component = components
            .iter()
            .find(|c| c.class_name == "PhysicsRigidBodyCharacterComponent")
            .context("Player has no character physics component")?;
        let path = component
            .properties
            .iter()
            .find(|p| p.name == "RB Hull File Path")
            .and_then(|p| p.value.as_str())
            .filter(|p| !p.is_empty())
            .context("Player has no authored collision resource")?;
        let key = |path: &str| {
            path.replace('\\', "/")
                .trim_start_matches('/')
                .to_ascii_lowercase()
        };
        let wanted = key(path);
        let mut candidates = files.iter().filter(|f| {
            let candidate = key(&f.path);
            candidate == wanted || candidate == format!("{wanted}_wii")
        });
        let resource = candidates
            .next()
            .with_context(|| format!("Missing player hull {path}"))?;
        ensure!(candidates.next().is_none(), "Ambiguous player hull {path}");
        let file = epic_mickey_lib_rs::hkx_file::HkxFile::load_bytes(&resource.data)
            .map_err(anyhow::Error::msg)?;
        let (hull, support) = super::character_hull::load_with_support(&file)
            .with_context(|| format!("Player collision resource {}", resource.path))?;
        Ok((resource.path.clone(), hull, support))
    }
    pub fn summary(&self) -> serde_json::Value {
        let transform = scene::get_entity_transform(self.entity(), &self.scene.objects);
        let components = scene::get_effective_components(self.entity(), &self.scene.objects);
        let pending = components
            .iter()
            .find(|c| c.class_name == "ChildComponent")
            .and_then(|c| c.properties.iter().find(|p| p.name == "Child Templates"))
            .map(|p| p.value.clone());
        serde_json::json!({"entity":self.entity().name,"marker":self.marker,"player_index":self.player_index,
            "position":transform.translation,"rotation":transform.rotation,"scale":transform.scale,
            "components":components.iter().map(|c|c.class_name.as_str()).collect::<Vec<_>>(),
            "child_templates_pending":pending,"world_lifecycle_initialized":false})
    }
}
