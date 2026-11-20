//! Effect texture parameters resolved against the mounted source assets.
//! Missing authored inputs and runtime-owned images remain explicit requests.
use anyhow::{ensure,Context,Result};
use std::{collections::BTreeMap,sync::Arc};
use epic_mickey_lib_rs::packfile::VirtualFile;
use super::{d3d9_bindings::Textures,d3d9_images::Source,d3d9_shader::{Effect,MaterialEffect,TechniqueBinding}};

fn key(path:&str)->Result<String> {
    let path=path.replace('\\',"/");
    ensure!(!path.contains(':')&&!path.split('/').any(|part|part==".."),"Unsafe texture resource path");
    Ok(path.split('/').filter(|part|!part.is_empty()&&*part!=".").collect::<Vec<_>>().join("/").to_ascii_lowercase())
}
#[derive(Debug)]
pub(super) struct Map {pub shader:bool,pub slot:u32,pub path:String}
#[derive(Debug)]
pub(super) enum Texture {
    Image {path:String,source:Arc<Source>},
    SceneObject {name:String,index:Option<i32>},
    MissingFile {path:String},
    MissingMap {shader:bool,slot:u32},
}
pub(super) struct Resources {pub effect:Arc<Effect>,pub technique:TechniqueBinding,pub textures:BTreeMap<usize,Option<Texture>>,pub mesh_time:bool}
impl std::fmt::Debug for Resources {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {f.debug_struct("EffectResources").field("effect_bytes",&self.effect.data.len()).field("texture_parameters",&self.textures.len()).finish()}
}
/// Scoped to the assets actually mounted by this level. Identical paths with
/// conflicting bytes are an error, never a basename/iteration-order choice.
pub(super) struct Catalog<'a> {
    files:BTreeMap<String,Vec<&'a VirtualFile>>,
    sources:BTreeMap<String,Result<Arc<Source>,String>>,
}
impl<'a> Catalog<'a> {
    pub fn new(files:&'a [VirtualFile])->Result<Self> {
        let mut index:BTreeMap<String,Vec<&VirtualFile>>=BTreeMap::new();
        for file in files {index.entry(key(&file.path)?).or_default().push(file);}
        Ok(Self {files:index,sources:BTreeMap::new()})
    }
    fn image(&mut self,path:&str)->Result<Option<Texture>> {
        let key=key(path)?;
        let Some(files)=self.files.get(&key) else{return Ok(None)};
        if !self.sources.contains_key(&key) {
            let source=(||->Result<Arc<Source>> {
                let first=files[0];ensure!(files.iter().skip(1).all(|f|f.data==first.data),"Conflicting mounted texture resource {path}");
                Ok(Arc::new(Source::discover(Arc::new(first.data.clone()))?))
            })().map_err(|e|format!("{path}: {e:#}"));
            self.sources.insert(key.clone(),source);
        }
        let source=self.sources[&key].as_ref().map_err(|e|anyhow::anyhow!("{e}"))?.clone();
        Ok(Some(Texture::Image {path:key,source}))
    }
    pub fn prepare(&mut self,selection:MaterialEffect,maps:&[Map])->Result<Resources> {
        let MaterialEffect {effect,binding:technique}=selection;
        // A duplicate map cannot be resolved safely by whichever Vec entry
        // happened to be appended first by the NIF importer.
        let mut slots=std::collections::BTreeSet::new();
        for map in maps {ensure!(slots.insert((map.shader,map.slot)),"Duplicate NIF material map slot");}
        let mut receiver=Receiver {catalog:self,maps};
        let textures=effect.resolve_textures(&mut receiver)?;
        let mesh_time=effect.requires_mesh_time(technique.technique)?;
        Ok(Resources {effect,technique,textures,mesh_time})
    }
    pub fn summary(&self)->serde_json::Value {
        serde_json::json!({"decoded_sources":self.sources.values().filter(|v|v.is_ok()).count(),"source_errors":self.sources.values().filter_map(|v|v.as_ref().err()).collect::<Vec<_>>()})
    }
}
struct Receiver<'a,'b,'c> {catalog:&'a mut Catalog<'b>,maps:&'c [Map]}
impl Receiver<'_, '_, '_> {
    fn map(&mut self,shader:bool,slot:u32)->Result<Option<Texture>> {
        let Some(map)=self.maps.iter().find(|m|m.shader==shader&&m.slot==slot) else{return Ok(Some(Texture::MissingMap {shader,slot}))};
        Ok(Some(match self.catalog.image(&map.path)? {Some(image)=>image,None=>Texture::MissingFile {path:map.path.clone()}}))
    }
}
impl Textures for Receiver<'_, '_, '_> {
    type Texture=Texture;
    fn named(&mut self,name:&str)->Result<Option<Texture>> {self.catalog.image(name)}
    fn shader_map(&mut self,index:u32)->Result<Option<Texture>> {self.map(true,index)}
    fn base_map(&mut self)->Result<Option<Texture>> {self.map(false,0)}
    fn object(&mut self,name:&str,index:Option<i32>)->Result<Option<Texture>> {Ok(Some(Texture::SceneObject {name:name.into(),index}))}
    fn other_map(&mut self,name:&str,index:Option<i32>)->Result<Option<Texture>> {
        let slot=match name.to_ascii_lowercase().as_str() {
            "dark"=>1,"detail"=>2,"gloss"=>3,"glow"=>4,"bump"=>5,"normal"=>6,"parallax"=>7,
            "decal"=>{let index=index.context("Decal map requires its authored index")?;ensure!(index>=0,"Negative decal map index");8u32.checked_add(index as u32).ok_or_else(||anyhow::anyhow!("Decal map index overflow"))?},
            _=>anyhow::bail!("Original texture map {name} requires its material owner"),
        };
        self.map(false,slot)
    }
}
impl Resources {
    pub fn summary(&self)->serde_json::Value {
        let mut images=0;let mut objects=Vec::new();let mut missing=Vec::new();
        for (parameter,texture) in &self.textures {match texture {
            Some(Texture::Image {..})=>images+=1,
            Some(Texture::SceneObject {name,index})=>objects.push(serde_json::json!({"parameter":parameter,"object":name,"index":index})),
            Some(Texture::MissingFile {path})=>missing.push(serde_json::json!({"parameter":parameter,"path":path})),
            Some(Texture::MissingMap {shader,slot})=>missing.push(serde_json::json!({"parameter":parameter,"shader_map":shader,"slot":slot})),
            None=>{},
        }}
        serde_json::json!({"technique":self.technique,"mesh_time_controller_required":self.mesh_time,"image_parameters":images,"scene_texture_requests":objects,"missing_resources":missing})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resource_keys_preserve_directories_and_reject_escaping_paths()->Result<()> {
        assert_eq!(key("/Effects\\A/Flame.NIF")?,"effects/a/flame.nif");
        assert_ne!(key("a/x.nif")?,key("b/x.nif")?);
        assert_eq!(key(r"\\handheld\\textures\\mickeyBrushNG_N_tex.nif")?,key("handheld/textures/mickeyBrushNG_N_tex.nif")?);
        assert_eq!(key("a//./b.nif")?,key("a/b.nif")?);
        assert!(key("a//../x.nif").is_err());
        assert!(key("../x.nif").is_err());assert!(key("D:/x.nif").is_err());Ok(())
    }
    #[test]
    fn absent_map_and_absent_file_remain_distinct_requests()->Result<()> {
        let mut catalog=Catalog::new(&[])?;
        let maps=[Map {shader:true,slot:2,path:"/a.nif".into()}];
        let mut receiver=Receiver {catalog:&mut catalog,maps:&maps};
        assert!(matches!(receiver.shader_map(0)?,Some(Texture::MissingMap {shader:true,slot:0})));
        assert!(matches!(receiver.shader_map(2)?,Some(Texture::MissingFile {..})));
        assert!(receiver.named("missing.nif")?.is_none());
        assert!(matches!(receiver.object("ProjectedShadowMap",Some(0))?,Some(Texture::SceneObject {index:Some(0),..})));Ok(())
    }
    #[test]
    fn conflicting_mounted_images_do_not_choose_the_first_file()->Result<()> {
        let files=[vec![1u8],vec![2u8]].into_iter().map(|bytes|VirtualFile::new(epic_mickey_lib_rs::packfile::EndianDependentString::new("nif ".into()),false,0,"a.nif".into(),bytes)).collect::<Vec<_>>();
        let mut catalog=Catalog::new(&files)?;
        let error=catalog.image("/A.NIF").unwrap_err().to_string();assert!(error.contains("Conflicting"));
        assert_eq!(catalog.sources.len(),1);assert!(catalog.image("a.nif").unwrap_err().to_string().contains("Conflicting"));Ok(())
    }
}
