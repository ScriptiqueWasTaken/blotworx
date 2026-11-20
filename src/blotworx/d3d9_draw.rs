//! Original PC effects at the shared viewport draw boundary. No scene defaults
//! are invented here: the scene owner must admit numeric and device inputs.
use anyhow::{ensure,Context,Result};
use eframe::glow::{self,HasContext};
use std::{collections::{BTreeMap,BTreeSet},sync::{Arc,Weak}};
use crate::render_support::{native_material::{Backend,Frame},nif::MaterialBindings,viewport3d::{GpuDrawCommand,GpuMeshHandle}};
use super::{d3d9_gpu as gpu,d3d9_images as images,d3d9_material::{Resources,Texture},d3d9_raster as raster,d3d9_sampling as sampling,d3d9_shader::{Effect,PassSamplers,TranslatedPass},d3d9_state,d3d9_vertices};

pub(super) type Materials=BTreeMap<usize,(Weak<MaterialBindings>,Result<Arc<Resources>,String>)>;
#[derive(Debug,Clone)]
pub(super) enum InputImage {
    /// Explicit known absence; never used to hide unresolved scene ownership.
    Null2d,
    Asset(Arc<images::Source>),
    /// Borrowed from the current scene pass (for example a projected shadow).
    /// The scene owner keeps it alive for this draw; this cache never deletes it.
    Runtime(sampling::ImageView),
}
#[derive(Debug,Clone)]
pub(super) struct AssignedTextureEffect {pub image:Option<InputImage>,pub projection:Option<super::d3d9_texture_effects::Projection>}
pub(super) struct Inputs {
    pub texture_precision:Option<super::pc_float::Precision>,
    pub lighting:Option<super::d3d9_globals::Lighting>,
    pub sh:Option<Arc<super::d3d9_sh::Snapshot>>,
    pub predefined:Option<super::d3d9_predefined::Cache>,
    /// Exact shader Object annotation and optional explicit ObjectIndex.
    /// Missing assignments stay errors; preview light ordering is unrelated.
    pub assigned_lights:Option<Arc<super::d3d9_light::State>>,
    pub object_lights:BTreeMap<(String,Option<i32>),super::d3d9_light::Light>,
    pub parameters:BTreeMap<String,Vec<u32>>,
    /// Only authored Attribute parameters can explicitly retain FXO defaults.
    pub attribute_defaults:BTreeSet<String>,
    pub images:BTreeMap<usize,InputImage>,
    pub texture_effects:Option<Arc<super::d3d9_texture_effects::State<AssignedTextureEffect>>>,
    pub device:raster::Device,
    pub samplers:BTreeMap<(bool,u32),sampling::Device>,
    pub target:raster::Target,pub coordinates:gpu::RasterCoordinates,
    pub precision:super::d3d9_preshader::TemporaryPrecision,
}
impl Inputs {
    fn texture_effect(&self,name:&str,index:Option<i32>)->Result<Option<&AssignedTextureEffect>> {
        use super::d3d9_texture_effects::Kind;
        let kind=Kind::named(name).with_context(||format!("Unconnected original scene texture {name}"))?;
        let effects=self.texture_effects.as_ref().with_context(||format!("Missing original scene texture ownership {name}"))?;
        let index=match kind {Kind::Environment|Kind::Fog=>index.unwrap_or(0),_=>index.context("Automatic texture-effect index requires registration-order ownership")?} as u32;
        Ok(effects.select(kind,index).map(|effect|&effect.value))
    }
}
pub(super) trait InputOwner:std::fmt::Debug+Send {
    fn fallback_only(&self)->bool {false}
    fn prepare(&mut self,resources:&Resources,draw:&GpuDrawCommand,frame:&Frame<'_>)->Result<Inputs>;
}
/// Explicit temporary owner while the scene-value adapter is under repair.
/// It never submits a zero-matrix or guessed-lighting original shader draw.
#[derive(Debug)]
pub(super) struct UnboundScene;
impl InputOwner for UnboundScene {
    fn fallback_only(&self)->bool {true}
    fn prepare(&mut self,resources:&Resources,_:&GpuDrawCommand,_:&Frame<'_>)->Result<Inputs> {
        anyhow::bail!("Original scene inputs pending for {}",resources.technique.shader_name)
    }
}
/// Live demo bridge: uses the shared viewport's actual camera/object matrices.
/// Original camera lifecycle, inverse-cache rounding, global-light selection and
/// pass initialization parity remain tracked separately. Missing authored or
/// runtime inputs still reject the draw and use the visible fallback.
#[derive(Debug)]
pub(super) struct ViewportScene {pub profiles:Option<Arc<std::sync::Mutex<super::render_profiles::Registry>>>}
impl InputOwner for ViewportScene {
    fn prepare(&mut self,_:&Resources,draw:&GpuDrawCommand,frame:&Frame<'_>)->Result<Inputs> {
        use super::d3d9_predefined::{self as predefined,Matrix};
        ensure!(frame.viewport[2]>0&&frame.viewport[3]>0,"Empty live renderer viewport");
        let lighting=if let Some(profiles)=&self.profiles {
            let mut state=profiles.lock().map_err(|_|anyhow::anyhow!("Render-profile state lock poisoned"))?;
            state.observe(frame.eye,super::pc_float::Precision::Extended)?;
            // This bridge submits the common viewport's main-scene pass.
            // The separate original toon offscreen/composite pass is pending.
            state.lighting(super::d3d9_globals::ScenePass::Main).cloned()
        }else{None};
        let source=draw.source_attributes.as_ref().context("Live renderer has no mesh-space transform")?.as_ref().map_err(|e|anyhow::anyhow!("{e}"))?;
        let mesh_to_model=std::array::from_fn(|i|source.mesh_to_model[i%4][i/4]);
        let world=predefined::multiply(&mesh_to_model,&draw.trans_mat);
        // GL [-w,+w] depth -> D3D [0,+w]. The shader bridge reverses this
        // at submission, so the common viewport and native draw coincide.
        let mut depth=[0.;16];for i in [0,5,15]{depth[i]=1.;}depth[10]=0.5;depth[14]=0.5;
        let projection=predefined::multiply(frame.projection,&depth);
        let direction=[-frame.view[2],-frame.view[6],-frame.view[10],0.];
        let mut cache=predefined::Cache::compose(world,*frame.view,projection,[frame.eye[0],frame.eye[1],frame.eye[2],1.],direction,[0.,1.,1.,0.])?;
        // Functional live bridge; these are mathematical inverses, not yet
        // a claim of original 32-bit D3DX inverse-cache bit equivalence.
        for kind in [Matrix::World,Matrix::View,Matrix::Projection,Matrix::WorldView,Matrix::ViewProjection,Matrix::WorldViewProjection] {
            let matrix=cache.matrices[&kind].value;cache.set_inverse(kind,live_inverse(matrix)?)?;
        }
        let mut device=raster::Device::native_initial()?;
        device.write(&[d3d9_state::State {state:7,value:1}])?;
        let sampler=sampling::Device::native_initial()?;
        let samplers=(0..16).map(|register|((false,register),sampler.clone())).chain((0..4).map(|register|((true,register),sampler.clone()))).collect();
        Ok(Inputs {texture_precision:Some(super::pc_float::Precision::Extended),lighting,sh:None,predefined:Some(cache),assigned_lights:None,object_lights:BTreeMap::new(),parameters:BTreeMap::new(),attribute_defaults:BTreeSet::new(),images:BTreeMap::new(),texture_effects:None,device,samplers,target:raster::Target {front_face:glow::CCW,depth_bias_units:None,extent:[frame.viewport[2] as u32,frame.viewport[3] as u32]},coordinates:gpu::RasterCoordinates {vpos_flip:[-1.,1.],clip_y:1.,d3d_depth:true},precision:super::d3d9_preshader::TemporaryPrecision::Double})
    }
}
fn live_inverse(matrix:[f32;16])->Result<[f32;16]> {
    let mut rows=[[0f64;8];4];
    for r in 0..4 {for c in 0..4 {rows[r][c]=matrix[r*4+c] as f64;}rows[r][r+4]=1.;}
    for c in 0..4 {
        let pivot=(c..4).max_by(|&a,&b|rows[a][c].abs().total_cmp(&rows[b][c].abs())).unwrap();
        ensure!(rows[pivot][c]!=0.,"Singular live renderer transform");rows.swap(c,pivot);
        let divisor=rows[c][c];for v in &mut rows[c]{*v/=divisor;}
        for r in 0..4 {if r!=c {let factor=rows[r][c];for k in 0..8 {rows[r][k]-=factor*rows[c][k];}}}
    }
    let result:[f32;16]=std::array::from_fn(|i|rows[i/4][i%4+4] as f32);
    ensure!(result.iter().all(|v|v.is_finite()),"Nonfinite live renderer inverse");Ok(result)
}

type PipelineKey=(usize,usize);
type GeometryKey=(usize,usize,usize,usize,usize);
type SamplerKey=(i32,sampling::Device);
struct Pass {source:Arc<TranslatedPass>,program:gpu::Program,samplers:PassSamplers,writes:Vec<d3d9_state::State>}
struct Pipeline {passes:Vec<Pass>}
impl Pipeline {
    unsafe fn new(gl:&glow::Context,resources:&Resources)->Result<Self> {
        let mut result=Self {passes:Vec::new()};
        let build=(||->Result<()> {
            let layout=resources.effect.layout.as_ref().map_err(|e|anyhow::anyhow!("{e}"))?;
            let technique=layout.techniques.get(resources.technique.technique).context("Registered technique was retired")?;
            for pass in &technique.passes {
                let source=Arc::new(resources.effect.translate_pass(&resources.technique.name,pass.name.as_deref().context("Original pass is unnamed")?)?);
                let samplers=resources.effect.sampler_bindings(&source)?;
                let writes=resources.effect.render_states(&source)?;
                let program=gpu::Program::link(gl,source.clone())?;
                result.passes.push(Pass {source,program,samplers,writes});
            }
            ensure!(!result.passes.is_empty(),"Registered effect has no passes");Ok(())
        })();
        if let Err(error)=build {result.destroy(gl);return Err(error);}Ok(result)
    }
    unsafe fn destroy(self,gl:&glow::Context){for pass in self.passes {pass.program.destroy(gl);}}
}
struct PipelineEntry {source:Weak<Effect>,value:Result<Pipeline,String>}
struct Geometry {vertices:Weak<Vec<f32>>,indices:Weak<Vec<u32>>,binding:d3d9_vertices::Binding}
struct Image {source:Weak<images::Source>,value:images::Image}
struct Sampler {last_seen:u64,value:sampling::Binding}
enum ReadyImage {Cached(usize),Runtime(sampling::ImageView),Null2d}
struct ReadyPass {geometry:GeometryKey,textures:Vec<(u32,SamplerKey,ReadyImage)>,device:raster::Device}
pub(super) struct Renderer {
    materials:Materials,owner:Box<dyn InputOwner>,pipelines:BTreeMap<PipelineKey,PipelineEntry>,
    geometry:BTreeMap<GeometryKey,Geometry>,images:BTreeMap<usize,Image>,samplers:BTreeMap<SamplerKey,Sampler>,
    scratch:gpu::Scratch,frame:u64,errors:BTreeMap<usize,String>,
}
#[derive(Debug,serde::Serialize)]
pub(super) struct Counts {pub materials:usize,pub pipelines:usize,pub geometry:usize,pub images:usize,pub samplers:usize,pub failed_materials:usize}
impl std::fmt::Debug for Renderer {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {f.debug_struct("OriginalPcRenderer").field("materials",&self.materials.len()).field("pipelines",&self.pipelines.len()).field("images",&self.images.len()).field("owner",&self.owner).finish()}
}
impl Renderer {
    pub fn counts(&self)->Counts {Counts {materials:self.materials.len(),pipelines:self.pipelines.len(),geometry:self.geometry.len(),images:self.images.len(),samplers:self.samplers.len(),failed_materials:self.errors.len()}}
    pub fn new(materials:Materials,owner:Box<dyn InputOwner>)->Self {
        Self {materials,owner,pipelines:BTreeMap::new(),geometry:BTreeMap::new(),images:BTreeMap::new(),samplers:BTreeMap::new(),scratch:gpu::Scratch::default(),frame:0,errors:BTreeMap::new()}
    }
    fn record_error(&mut self,identity:usize,error:&str) {
        if self.errors.get(&identity).is_none_or(|old|old!=error) {
            log::warn!("Original PC material fallback: {error}");
            self.errors.insert(identity,error.to_owned());
        }
    }
    fn numeric_admission(resources:&Resources,pass:&TranslatedPass,draw:&GpuDrawCommand,inputs:&mut Inputs)->Result<()> {
        let layout=resources.effect.layout.as_ref().map_err(|e|anyhow::anyhow!("{e}"))?;
        let names=resources.effect.numeric_inputs(pass)?;
        for name in names {
            let index=layout.parameters.iter().position(|p|p.value.name.as_deref()==Some(name)).context("Numeric scene parameter is absent")?;
            if let Some(kind)=resources.effect.binding_annotations[index].text("VarType")? {
                if kind.eq_ignore_ascii_case("Global")&&!inputs.parameters.contains_key(name) {
                    if let Some(value)=inputs.lighting.as_ref().and_then(|lighting|lighting.resolve(name)) {
                        inputs.parameters.insert(name.into(),value.words(&layout.parameters[index].value)?);
                    }
                }
                if kind.eq_ignore_ascii_case("Global")&&name=="g_renderTargetDimensions"&&!inputs.parameters.contains_key(name) {
                    let value=super::d3d9_predefined::Value::Vector(super::d3d9_globals::target_dimensions(inputs.target.extent)?);
                    inputs.parameters.insert(name.into(),value.words(&layout.parameters[index].value)?);
                }
                if kind.eq_ignore_ascii_case("Object")&&!inputs.parameters.contains_key(name) {
                    let annotations=&resources.effect.binding_annotations[index];
                    if let Some(object)=annotations.text("Object")? {
                        let key=(object.to_owned(),annotations.integer("ObjectIndex")?);
                        if super::d3d9_texture_effects::Kind::named(object).is_some() {
                            let property=annotations.text("ObjectProperty")?.context("Texture-effect property is absent")?;
                            if super::d3d9_texture_effects::Projection::handles(property) {
                                let source=inputs.texture_effect(object,key.1)?.map(|effect|effect.projection.as_ref().context("Assigned texture effect lost its projection properties")).transpose()?;
                                let value=super::d3d9_texture_effects::Projection::resolve(source,property).unwrap();
                                inputs.parameters.insert(name.into(),value.words(&layout.parameters[index].value)?);
                            }
                        }
                        if let Some(light)=inputs.object_lights.get(&key) {
                            let property=annotations.text("ObjectProperty")?.context("Light property is absent")?;
                            if let Some(words)=light.words(property,annotations.text("Space")?,&layout.parameters[index].value)? {
                                inputs.parameters.insert(name.into(),words);
                            }
                        }
                        if !inputs.parameters.contains_key(name) {
                            if let Some(state)=&inputs.assigned_lights {
                                let property=annotations.text("ObjectProperty")?.context("Light property is absent")?;
                                if let Some(words)=state.words(object,key.1,property,annotations.text("Space")?,&layout.parameters[index].value)? {
                                    inputs.parameters.insert(name.into(),words);
                                }
                            }
                        }
                    }
                }
                if kind.eq_ignore_ascii_case("Attribute")&&!inputs.parameters.contains_key(name) {
                    if let Some(snapshot)=&inputs.sh {
                        if let Some(words)=snapshot.words(name,&layout.parameters[index].value)? {inputs.parameters.insert(name.into(),words);}
                    }
                }
                if kind.eq_ignore_ascii_case("Attribute")&&!inputs.parameters.contains_key(name) {
                    if let Some(attributes)=draw.material.as_ref().and_then(|material|material.attributes.as_ref()) {
                        let attributes=attributes.as_ref().map_err(|error|anyhow::anyhow!("{error}"))?;
                        if let Some(attribute)=attributes.get(name) {
                            inputs.parameters.insert(name.into(),super::d3d9_attributes::words(attribute,&layout.parameters[index].value)?);
                        }
                    }
                }
                if kind.eq_ignore_ascii_case("Predefined")&&!inputs.parameters.contains_key(name) {
                    let mapping=resources.effect.binding_annotations[index].text("DefinedMapping")?.context("Predefined mapping name is absent")?;
                    if mapping.eq_ignore_ascii_case("TexCoordGenProjMatrix") {
                        let bound=draw.projection_bound.context("TexCoordGenProjMatrix requires the mesh's actual world-bound center")?;
                        let cache=inputs.predefined.as_ref().context("TexCoordGenProjMatrix requires camera cache ownership")?;
                        let precision=inputs.texture_precision.context("TexCoordGenProjMatrix requires native precision ownership")?;
                        inputs.parameters.insert(name.into(),cache.texgen_projection(bound,precision)?.words(&layout.parameters[index].value)?);
                    }
                    if let Some(material)=&draw.material {
                        if let Some(value)=super::d3d9_texture_transform::resolve(material,mapping,resources.effect.binding_annotations[index].integer("NTMIndex")?,inputs.texture_precision)? {
                            inputs.parameters.insert(name.into(),value.words(&layout.parameters[index].value)?);
                        }
                    }
                    if mapping.eq_ignore_ascii_case("time") {
                        if let Some(time)=draw.shader_time.as_ref().and_then(|clock|clock.value()) {
                            inputs.parameters.insert(name.into(),super::d3d9_attributes::shader_time_value(time,&layout.parameters[index].value)?);
                        } else if let Some(attributes)=draw.material.as_ref().and_then(|material|material.attributes.as_ref()) {
                            let attributes=attributes.as_ref().map_err(|error|anyhow::anyhow!("{error}"))?;
                            if let Some(value)=super::d3d9_attributes::shader_time(attributes,&layout.parameters[index].value)? {inputs.parameters.insert(name.into(),value);}
                        }
                    }
                    if let Some(cache)=&inputs.predefined {
                        if !inputs.parameters.contains_key(name) {if let Some(value)=cache.resolve(mapping)? {inputs.parameters.insert(name.into(),value.words(&layout.parameters[index].value)?);}}
                    }
                }
                let admitted_default=kind.eq_ignore_ascii_case("Attribute")&&inputs.attribute_defaults.contains(name);
                ensure!(inputs.parameters.contains_key(name)||admitted_default,"Missing original {kind} input {name}");
            }
        }
        Ok(())
    }
    unsafe fn execute(&mut self,gl:&glow::Context,resources:&Arc<Resources>,draw:&GpuDrawCommand,mesh:GpuMeshHandle,mut inputs:Inputs)->Result<()> {
        // Resolve scene ownership before allocating GPU state, and prepare
        // every pass before submitting any geometry for this object.
        ensure!(!self.owner.fallback_only(),"Scene owner is restricted to the legacy fallback");
        ensure!(draw.v_data.len()%12==0&&mesh.index_count>=0&&mesh.index_count as usize==draw.indices.len(),"Original draw geometry differs from shared buffers");
        let key=(Arc::as_ptr(&resources.effect) as usize,resources.technique.technique);
        if !self.pipelines.contains_key(&key) {
            let value=Pipeline::new(gl,resources).map_err(|e|format!("{e:#}"));
            self.pipelines.insert(key,PipelineEntry {source:Arc::downgrade(&resources.effect),value});
        }
        let pipeline=self.pipelines[&key].value.as_ref().map_err(|e|anyhow::anyhow!("{e}"))?;
        let mut device=inputs.device.clone();let mut ready=Vec::with_capacity(pipeline.passes.len());
        for (index,pass) in pipeline.passes.iter().enumerate() {
            Self::numeric_admission(resources,&pass.source,draw,&mut inputs)?;
            let banks=resources.effect.constant_banks(&pass.source,&inputs.parameters,inputs.precision)?;
            device.write(&pass.writes)?;device.validate_target(&inputs.target)?;
            pass.program.validate_alpha_test(&device.alpha_test()?)?;
            // Omitted sampler writes inherit earlier passes, including when a
            // different sampler parameter occupies the same hardware register.
            let mut textures=Vec::new();
            for (vertex,routes,registers) in [(true,&pass.samplers.vertex,&pass.program.vertex_samplers),(false,&pass.samplers.pixel,&pass.program.pixel_samplers)] {
                for route in routes {
                    let sampler=registers.iter().find(|s|s.register==route.register as u32).context("Original shader sampler interface differs")?;
                    let parameter=route.texture_parameter;
                    if parameter.is_none() {
                        ensure!(!vertex&&sampler.dimension==0,"Null D3D texture requires its verified stage/dimension adapter");
                        let state=inputs.samplers.get_mut(&(vertex,sampler.register)).context("Original sampler device snapshot is absent")?;state.write(&route.states)?;let state=state.clone();
                        let sampler_key=(sampler.dimension,state.clone());
                        if !self.samplers.contains_key(&sampler_key){self.samplers.insert(sampler_key.clone(),Sampler {last_seen:self.frame,value:sampling::Binding::new(gl,&state,sampler.dimension)?});}
                        let binding=self.samplers.get_mut(&sampler_key).unwrap();binding.last_seen=self.frame;binding.value.bind_null_2d(gl,sampler.unit)?;
                        textures.push((sampler.unit,sampler_key,ReadyImage::Null2d));continue;
                    }
                    let parameter=parameter.unwrap();
                    let input_image=if let Some(image)=inputs.images.get(&parameter) {image.clone()}else {match resources.textures.get(&parameter) {
                        Some(Some(Texture::Image {source,..}))=>InputImage::Asset(source.clone()),
                        Some(Some(Texture::SceneObject {name,index}))=>{
                            inputs.texture_effect(name,*index)?.and_then(|effect|effect.image.clone()).unwrap_or(InputImage::Null2d)
                        },
                        Some(Some(Texture::MissingFile {path}))=>anyhow::bail!("Missing original texture {path}"),
                        Some(Some(Texture::MissingMap {shader,slot}))=>anyhow::bail!("Missing original NIF map shader={shader},slot={slot}"),
                        _=>anyhow::bail!("Original texture parameter {parameter} is unbound"),
                    }};
                    let state=inputs.samplers.get_mut(&(vertex,sampler.register)).context("Original sampler device snapshot is absent")?;state.write(&route.states)?;let state=state.clone();
                    let sampler_key=(sampler.dimension,state.clone());
                    if !self.samplers.contains_key(&sampler_key){self.samplers.insert(sampler_key.clone(),Sampler {last_seen:self.frame,value:sampling::Binding::new(gl,&state,sampler.dimension)?});}
                    self.samplers.get_mut(&sampler_key).unwrap().last_seen=self.frame;
                    let image=match input_image {
                        InputImage::Null2d=>{
                            ensure!(!vertex&&sampler.dimension==0,"Known-null scene texture requires a verified pixel 2D sampler");
                            self.samplers[&sampler_key].value.bind_null_2d(gl,sampler.unit)?;
                            textures.push((sampler.unit,sampler_key,ReadyImage::Null2d));continue;
                        },
                        InputImage::Asset(source)=>{
                            let image_key=Arc::as_ptr(&source) as usize;
                            if !self.images.contains_key(&image_key){let image=images::Image::new(gl,&source)?;self.images.insert(image_key,Image {source:Arc::downgrade(&source),value:image});}
                            if state.needs_srgb()? {self.images.get_mut(&image_key).unwrap().value.ensure_srgb(gl)?;}
                            ReadyImage::Cached(image_key)
                        },
                        InputImage::Runtime(view)=>ReadyImage::Runtime(view),
                    };
                    // Validate view/target and stage binding before any pass draws.
                    let view=match &image {ReadyImage::Cached(key)=>self.images[key].value.view,ReadyImage::Runtime(view)=>*view,ReadyImage::Null2d=>unreachable!()};
                    self.samplers[&sampler_key].value.bind(gl,sampler.unit,&view)?;
                    textures.push((sampler.unit,sampler_key,image));
                }
            }
            let geometry_key=(Arc::as_ptr(&draw.v_data) as usize,Arc::as_ptr(&draw.indices) as usize,key.0,key.1,index);
            let source=match &draw.source_attributes {Some(Ok(source))=>Some(source),Some(Err(error)) if pass.program.attributes.iter().any(|a|matches!(a.usage,0|3|6|7))=>anyhow::bail!("Original mesh-space streams: {error}"),_=>None};
            if let Some(geometry)=self.geometry.get_mut(&geometry_key) {geometry.binding.refresh(gl,mesh.vbo,mesh.ebo,draw.v_data.len()/12,source,draw.extra_uvs.as_deref())?;}
            else {let binding=d3d9_vertices::Binding::new_mesh_space(gl,&pass.program,mesh.vbo,mesh.ebo,draw.v_data.len()/12,source,draw.extra_uvs.as_deref())?;self.geometry.insert(geometry_key,Geometry {vertices:Arc::downgrade(&draw.v_data),indices:Arc::downgrade(&draw.indices),binding});}
            pass.program.upload(gl,&banks,&inputs.coordinates,&mut self.scratch)?;
            ready.push(ReadyPass {geometry:geometry_key,textures,device:device.clone()});
        }
        for (pass,ready) in pipeline.passes.iter().zip(ready) {
            gl.use_program(Some(pass.program.handle));
            let alpha=ready.device.submit(gl,&inputs.target)?;pass.program.upload_alpha_test(gl,&alpha)?;
            for (unit,sampler,image) in ready.textures {let view=match image {ReadyImage::Cached(key)=>self.images[&key].value.view,ReadyImage::Runtime(view)=>view,ReadyImage::Null2d=>{self.samplers[&sampler].value.bind_null_2d(gl,unit)?;continue;}};self.samplers[&sampler].value.bind(gl,unit,&view)?;}
            gl.bind_vertex_array(Some(self.geometry[&ready.geometry].binding.vao));
            gl.draw_elements(glow::TRIANGLES,mesh.index_count,glow::UNSIGNED_INT,0);
        }
        Ok(())
    }
}
impl Backend for Renderer {
    fn allows_linear_culling(&self,_:&GpuDrawCommand)->bool {self.owner.fallback_only()}
    fn handles(&self,draw:&GpuDrawCommand)->bool {draw.material.as_ref().is_some_and(|m|self.materials.contains_key(&(Arc::as_ptr(m) as usize)))}
    unsafe fn begin_frame(&mut self,gl:&glow::Context) {
        self.frame=self.frame.wrapping_add(1);self.materials.retain(|_,(source,_)|source.strong_count()!=0);
        self.errors.retain(|key,_|self.materials.contains_key(key));
        let live=self.materials.values().filter_map(|(_,value)|value.as_ref().ok()).map(|r|(Arc::as_ptr(&r.effect) as usize,r.technique.technique)).collect::<BTreeSet<_>>();
        self.pipelines.retain(|key,entry|{if live.contains(key)&&entry.source.strong_count()!=0 {true}else {if let Ok(pipeline)=std::mem::replace(&mut entry.value,Err("retired".into())){pipeline.destroy(gl);}false}});
        let retired=self.geometry.iter().filter(|(key,g)|g.vertices.strong_count()==0||g.indices.strong_count()==0||!self.pipelines.contains_key(&(key.2,key.3))).map(|(key,_)|*key).collect::<Vec<_>>();
        for key in retired {self.geometry.remove(&key).unwrap().binding.destroy(gl);}
        let retired=self.images.iter().filter(|(_,image)|self.materials.is_empty()||image.source.strong_count()==0).map(|(key,_)|*key).collect::<Vec<_>>();
        for key in retired {self.images.remove(&key).unwrap().value.destroy(gl);}
        let retired=self.samplers.iter().filter(|(_,s)|self.materials.is_empty()||s.last_seen!=self.frame.wrapping_sub(1)).map(|(key,_)|key.clone()).collect::<Vec<_>>();
        for key in retired {self.samplers.remove(&key).unwrap().value.destroy(gl);}
    }
    unsafe fn draw(&mut self,gl:&glow::Context,draw:&GpuDrawCommand,mesh:GpuMeshHandle,frame:&Frame<'_>)->Result<bool,String> {
        let Some(material)=draw.material.as_ref() else{return Ok(false)};
        let Some((_,resources))=self.materials.get(&(Arc::as_ptr(material) as usize)) else{return Ok(false)};
        let identity=Arc::as_ptr(material) as usize;
        let prepared=(||->Result<_ ,String> {
            let resources=resources.as_ref().map_err(Clone::clone)?.clone();
            let inputs=self.owner.prepare(&resources,draw,frame).map_err(|e|format!("{}: {e:#}",resources.technique.shader_name))?;
            Ok((resources,inputs))
        })();
        let (resources,inputs)=match prepared {Ok(value)=>value,Err(error)=>{self.record_error(identity,&error);return Err(error);}};
        let front=gl.get_parameter_i32(glow::FRONT_FACE) as u32;
        let result=self.execute(gl,&resources,draw,mesh,inputs).map(|_|true).map_err(|e|format!("{}: {e:#}",resources.technique.shader_name));
        for unit in 0..20 {gl.bind_sampler(unit,None);}
        gl.active_texture(glow::TEXTURE0);gl.bind_vertex_array(None);gl.use_program(None);gl.front_face(front);
        gl.disable(glow::STENCIL_TEST);gl.stencil_mask(u32::MAX);gl.disable(glow::POLYGON_OFFSET_FILL);gl.color_mask(true,true,true,true);
        match &result {Ok(_)=>{self.errors.remove(&identity);},Err(error)=>self.record_error(identity,error)}
        result
    }
    unsafe fn destroy(&mut self,gl:&glow::Context) {
        for (_,geometry) in std::mem::take(&mut self.geometry){geometry.binding.destroy(gl);}
        for (_,image) in std::mem::take(&mut self.images){image.value.destroy(gl);}
        for (_,sampler) in std::mem::take(&mut self.samplers){sampler.value.destroy(gl);}
        for (_,pipeline) in std::mem::take(&mut self.pipelines){if let Ok(pipeline)=pipeline.value {pipeline.destroy(gl);}}
        self.materials.clear();self.errors.clear();
    }
}
