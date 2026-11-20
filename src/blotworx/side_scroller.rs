//! Functional demo adapter for authored side-scrolling cameras. Not a retail
//! camera-generator reconstruction: smoothing, limit markers and control-point
//! transitions remain compatibility work. Original assets/flag meanings stay intact.
use super::{assets::{World,RuntimeEntity}, physics::Player};
use anyhow::Result;
#[derive(Default, serde::Serialize)]
pub struct State {
    pub enabled: bool,
    pub owner: Option<usize>,
    pub plane: Option<([f32;3], [f32;3])>,
    respawns: u32,
}
fn setting<'a>(entity: &'a RuntimeEntity, key: &str) -> Option<&'a serde_json::Value> {
    entity.components.iter().find(|c| c.class_name=="CameraOverrideComponent")
        .and_then(|c| c.properties.iter().find(|p|p.name==key))
        .map(|p| &p.value)
}
fn number(entity: &RuntimeEntity, key: &str, default: f32) -> f32 {
    setting(entity, key).and_then(serde_json::Value::as_f64).map(|v|v as f32)
        .filter(|v|v.is_finite()).unwrap_or(default)
}
pub fn project(vector: [f32;3], normal: [f32;3]) -> [f32;3] {
    let depth: f32=vector.iter().zip(normal).map(|(a,b)|a*b).sum();
    std::array::from_fn(|i|vector[i]-depth*normal[i])
}
impl World {
    fn side_scroll_camera(&self, index: usize) -> Option<&RuntimeEntity> {
        self.entities.get(index).filter(|entity| {
            !self.removed_entities.contains(&index)
                && self.entity_scene_active(index)
                && number(entity, "SideScrollDistance", 0.) > 0.
        })
    }
    pub(super) fn side_scroll_disables(&self, key: &str) -> bool {
        self.side_scroller.enabled && self.side_scroller.owner
            .and_then(|owner| self.side_scroll_camera(owner))
            .and_then(|entity| setting(entity, key))
            .and_then(serde_json::Value::as_bool).unwrap_or(false)
    }
    pub(super) fn prepare_side_scroller(&mut self, player: &mut Player) {
        if !self.side_scroller.enabled {
            self.side_scroller.plane=None;
            player.side_scroll_plane=None;
            return;
        }
        // A cinematic or an unloaded zone can still have a camera override
        // handle. Only a live side-scroll camera can own the gameplay plane.
        self.side_scroller.owner = self.demo_story.camera_override
            .filter(|&owner| self.side_scroll_camera(owner).is_some())
            .or_else(|| self.side_scroller.owner.filter(|&owner| self.side_scroll_camera(owner).is_some()))
            .or_else(|| (0..self.entities.len()).find(|&index| self.side_scroll_camera(index)
                .is_some_and(|entity| number(entity, "SideScrollActive", 2.) == 0.)));
        let yaw=self.side_scroller.owner.and_then(|i|self.side_scroll_camera(i))
            .map(|e|number(e,"SideScrollYaw",0.)).unwrap_or(0.).to_radians();
        let normal=[yaw.sin(),0.,yaw.cos()];
        if self.side_scroller.respawns!=player.respawns {
            self.side_scroller.plane=None;
            self.side_scroller.respawns=player.respawns;
        }
        if self.side_scroller.plane.is_some_and(|(_, previous)| previous != normal) {
            self.side_scroller.plane = None;
        }
        let plane=self.side_scroller.plane.get_or_insert((player.position,normal));
        player.side_scroll_plane=Some(*plane);
    }
    pub(super) fn side_scroller_frame(&self) -> Result<Option<super::nif_camera::Frame>> {
        if !self.side_scroller.enabled {return Ok(None)}
        let Some(owner)=self.side_scroller.owner.and_then(|i|self.side_scroll_camera(i)) else {return Ok(None)};
        let position: [f32;3]=self.entities[self.player_entity].transform[12..15].try_into().unwrap();
        let yaw=number(owner,"SideScrollYaw",0.).to_radians();
        let pitch=number(owner,"SideScrollPitch",0.).to_radians();
        let (sy,cy)=yaw.sin_cos(); let (sp,cp)=pitch.sin_cos();
        let direction=[-sy*cp,-sp,-cy*cp];
        let left=[-cy,0.,sy]; let up=[-sy*sp,cp,-cy*sp];
        let distance=number(owner,"SideScrollDistance",14.).max(0.1);
        let height=number(owner,"SideScrollOffset",0.)+number(owner,"SideScrollHeight",0.);
        let eye=std::array::from_fn(|i|position[i]+if i==1 {height}else{0.}-direction[i]*distance);
        let rotation=std::array::from_fn(|i|[left[i/3],up[i/3],direction[i/3]][i%3]);
        let fov=number(owner,"FOV",0.);
        let fov=if fov>0. {fov}else{self.camera_lens.field_of_view};
        super::nif_camera::Frame::from_information(super::camera_base::Information {
            transform:super::native_hk_transform::from_entity(rotation,eye),
            field_of_view:fov,focus:[position[0],position[1],position[2],0.],
        },self.camera_lens.sample(fov)).map(Some)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn plane_projection_retains_vertical_and_horizontal_motion() {
        assert_eq!(project([3.,4.,5.],[0.,0.,1.]),[3.,4.,0.]);
        let n=[0.6,0.,0.8];let v=project([3.,4.,5.],n);
        assert!((v[0]*n[0]+v[2]*n[2]).abs()<1e-6);
        assert_eq!(v[1],4.);
    }
}
