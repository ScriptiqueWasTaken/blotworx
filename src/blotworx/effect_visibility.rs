//! Direct NiVisController playback for effect model subtrees.
use super::{brush_particles::{ActivationTrack,ControllerTime},scene_bounds::{Controller,Hierarchy}};
use anyhow::{Context,Result,ensure};
use epic_mickey_lib_rs::nif_file::{NifFile,NiObject};
use std::collections::BTreeMap;
#[derive(Clone)]
pub struct Playback {
    controller:Controller,
    track:ActivationTrack,
    clock:ControllerTime,
}
#[derive(Clone,Default)]
pub struct Runtime {
    controls:BTreeMap<u32,Playback>,
    visible:BTreeMap<u32,bool>,
}
impl Runtime {
    pub fn load(nif:&NifFile,hierarchy:&Hierarchy)->Result<Self> {
        let mut result=Self::default();
        for (&node,source) in &hierarchy.nodes {
            result.visible.insert(node,source.flags&1==0);
            for controller in &source.controllers {
                if controller.class_name!="NiVisController" {continue;}
                let Some(NiObject::Unknown(raw))=nif.blocks.get(controller.block as usize) else {anyhow::bail!("Missing visibility controller")};
                ensure!(raw.raw_data.len()==30,"Invalid NiVisController layout");
                let bytes=raw.raw_data[26..30].try_into()?;
                let interpolator=if nif.source_is_little_endian {u32::from_le_bytes(bytes)} else {u32::from_be_bytes(bytes)};
                ensure!(controller.target.is_some_and(|target|hierarchy.nodes.contains_key(&target)),"Visibility target outside effect subtree");
                result.controls.insert(controller.block,Playback {controller:controller.clone(),track:ActivationTrack::load(nif,interpolator)?,clock:Default::default()});
            }
        }
        Ok(result)
    }
    pub fn owns(&self,id:u32)->bool {self.controls.contains_key(&id)}
    pub fn start(&mut self,id:u32)->Result<()> {
        let playback=self.controls.get_mut(&id).context("Visibility controller missing")?;
        playback.controller.flags|=8;
        playback.clock.reset();
        Ok(())
    }
    pub fn stop(&mut self,id:u32)->Result<()> {
        self.controls.get_mut(&id).context("Visibility controller missing")?.controller.flags&=!8;
        Ok(())
    }
    pub fn update(&mut self,time:f32)->Result<()> {
        for playback in self.controls.values_mut() {
            let c=&playback.controller;
            if c.flags&8==0 || c.flags&0x20!=0 {continue;}
            let time=playback.clock.scale_parameters(time,c.flags,c.frequency,c.phase,c.start,c.stop)?;
            self.visible.insert(c.target.unwrap(),playback.track.sample(time)?);
        }
        Ok(())
    }
    pub fn visible(&self,path:&[u32])->bool {path.iter().all(|node|self.visible.get(node).copied().unwrap_or(true))}
}
