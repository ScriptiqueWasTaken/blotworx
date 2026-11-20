//! Original mesh-space streams, retained separately from the common baked
//! model-space buffer. Raw component bytes and format/endianness stay intact.
use anyhow::{ensure,Context,Result};
use epic_mickey_lib_rs::nif_file::{NifFile,NiMeshData,NiObject};
use std::{collections::BTreeSet,sync::Arc};
#[derive(Clone,Debug)]
pub(crate) struct Stream {pub semantic:String,pub index:u32,pub format:u32,pub bytes:Arc<Vec<u8>>}
#[derive(Clone,Debug)]
pub(crate) struct Source {
    pub streams:Arc<Vec<Stream>>,pub vertices:usize,pub little_endian:bool,
    /// Row-major source mesh-to-model transform. These bytes are not already
    /// deformed/model-space vectors and must not be bound as though they were.
    pub mesh_to_model:[[f32;4];4],
    pub pose:Option<Arc<dyn DirectionPose>>,
}
/// A pose snapshot owns its inputs. Render callbacks may outlive the current
/// simulation tick, so they must never borrow the actor's mutable palette.
pub(crate) trait DirectionPose:std::fmt::Debug+Send+Sync {
    fn deform(&self,vectors:&mut [[f32;4]])->Result<()>;
    fn deform_positions(&self,_:&mut [[f32;4]])->Result<()> {anyhow::bail!("Native mesh position pose requires its point-deformation owner")}
}
struct Layout {stride:usize,offset:usize,width:usize,start:usize,count:usize}
fn gather(buffer:&[u8],layout:&Layout,sources:&[u32])->Result<Vec<u8>> {
    ensure!(layout.width>0&&layout.offset.checked_add(layout.width).is_some_and(|end|end<=layout.stride),"Invalid auxiliary attribute layout");
    let range=|source:u32|->Result<std::ops::Range<usize>> {
        ensure!((source as usize)<layout.count,"Auxiliary attribute source index outside region");
        let index=layout.start.checked_add(source as usize).context("Auxiliary attribute vertex overflow")?;
        let first=index.checked_mul(layout.stride).and_then(|v|v.checked_add(layout.offset)).context("Auxiliary attribute byte offset overflow")?;
        let end=first.checked_add(layout.width).context("Auxiliary attribute byte range overflow")?;
        ensure!(end<=buffer.len(),"Auxiliary attribute outside source buffer");Ok(first..end)
    };
    // Validate before allocation, including repeated and seam-split vertices.
    for &source in sources {range(source)?;}
    let capacity=sources.len().checked_mul(layout.width).context("Auxiliary attribute size overflow")?;
    let mut bytes=Vec::new();bytes.try_reserve_exact(capacity).context("Auxiliary attribute allocation failed")?;
    for &source in sources {bytes.extend_from_slice(&buffer[range(source)?]);}
    Ok(bytes)
}
fn is_auxiliary(name:&str,index:u32)->Result<bool> {
    // Vertex inputs and modifier/instance bookkeeping have different index
    // domains. Morph targets/weights remain with the existing morph owner.
    let vertex=match name {
        "POSITION"|"NORMAL"|"BINORMAL"|"TANGENT"|"TEXCOORD"|"BLENDWEIGHT"|"BLENDINDICES"|"COLOR"|"PSIZE"|"TESSFACTOR"|"DEPTH"|"FOG"|"POSITIONT"|"SAMPLE"=>true,
        "INDEX"|"DISPLAYLIST"|"BONEMATRICES"|"BONE_PALETTE"|"POSITION_BP"|"NORMAL_BP"|"BINORMAL_BP"|"TANGENT_BP"|"MORPHWEIGHTS"|"NORMALSHAREINDEX"|"NORMALSHAREGROUP"|"TRANSFORMS"|"INSTANCETRANSFORMS"=>false,
        name if name.starts_with("MORPH_")=>false,
        _=>anyhow::bail!("Mesh semantic {name} requires its stream owner"),
    };
    Ok(vertex&&name!="TEXCOORD"&&!matches!((name,index),("POSITION",0)|("NORMAL",0)|("COLOR",0)))
}
pub(crate) fn extract(nif:&NifFile,mesh:&NiMeshData,sources:Option<&[u32]>,mesh_to_model:[[f32;4];4])->Result<Option<Source>> {
    let mut streams=Vec::new();let mut names=BTreeSet::new();
    let display_list=mesh.stream_refs.iter().any(|r|r.semantics.iter().any(|s|s.name=="DISPLAYLIST"));
    for reference in &mesh.stream_refs {
        for (component,semantic) in reference.semantics.iter().enumerate(){
            let primary=matches!((semantic.name.as_str(),semantic.index),("POSITION",0)|("NORMAL",0));
            let auxiliary=is_auxiliary(&semantic.name,semantic.index)?;
            if !auxiliary&&!primary{continue;}
            // Legacy display lists have independent GX indices; keep their
            // existing common geometry path until that native adapter exists.
            if primary&&(display_list||sources.is_none()){continue;}
            ensure!(!display_list,"Display-list auxiliary attributes require independent attribute indices");
            ensure!(reference.is_per_instance==0,"Auxiliary per-instance stream requires its instance mapping");
            let sources=sources.context("Auxiliary attributes require original vertex indices")?;
            ensure!(reference.submesh_region_indices.len()<=1,"Auxiliary multi-region source mapping requires its adapter");
            let Some(NiObject::NiDataStream(stream))=nif.blocks.get(reference.stream_link as usize) else{anyhow::bail!("Auxiliary attribute stream is absent")};
            let region=stream.regions.get(reference.submesh_region_indices.first().copied().unwrap_or(0) as usize).context("Auxiliary attribute region is absent")?;
            let sizes=stream.component_formats.iter().map(|format|((format>>16)&255) as usize*((format>>8)&255) as usize).collect::<Vec<_>>();
            let stride=sizes.iter().try_fold(0usize,|sum,&n|sum.checked_add(n)).context("Auxiliary stream stride overflow")?;
            let offset=sizes.get(..component).context("Auxiliary semantic exceeds stream formats")?.iter().try_fold(0usize,|sum,&n|sum.checked_add(n)).context("Auxiliary component offset overflow")?;
            let width=*sizes.get(component).context("Auxiliary component format is absent")?;
            let layout=Layout {stride,offset,width,start:region.start_index as usize,count:region.count as usize};
            ensure!(names.insert((semantic.name.clone(),semantic.index)),"Duplicate auxiliary attribute semantic");
            streams.push(Stream {semantic:semantic.name.clone(),index:semantic.index,format:stream.component_formats[component],bytes:Arc::new(gather(&stream.buffer,&layout,sources)?)});
        }
    }
    Ok(if streams.is_empty(){None}else{Some(Source {streams:Arc::new(streams),vertices:sources.unwrap().len(),little_endian:nif.source_is_little_endian,mesh_to_model,pose:None})})
}
impl Source {
    /// Original shader object space, before the cached mesh-to-model bake.
    /// A pose operates in that same space; no inverse bake/round trip is used.
    pub fn mesh_vectors(&self,name:&str,index:u32)->Result<Vec<[f32;4]>> {
        ensure!(matches!(name,"POSITION"|"NORMAL"|"TANGENT"|"BINORMAL"),"Native semantic needs its vector adapter");
        let mut vectors=self.float_vectors(name,index)?;
        if let Some(pose)=&self.pose {if name=="POSITION" {pose.deform_positions(&mut vectors)?;}else{pose.deform(&mut vectors)?;}}
        ensure!(vectors.len()==self.vertices&&vectors.iter().all(|v|v[..3].iter().all(|v|v.is_finite())),"Invalid native mesh-space pose");
        Ok(vectors)
    }
    pub fn model_vectors(&self,name:&str,index:u32)->Result<Vec<[f32;4]>> {
        ensure!(matches!(name,"TANGENT"|"BINORMAL"),"Auxiliary semantic needs its coordinate-space adapter");
        ensure!(self.mesh_to_model.iter().flatten().all(|v|v.is_finite())&&self.mesh_to_model[3]==[0.,0.,0.,1.],"Invalid auxiliary mesh-to-model transform");
        let mut vectors=self.float_vectors(name,index)?;
        if let Some(pose)=&self.pose {pose.deform(&mut vectors)?;}
        ensure!(vectors.len()==self.vertices,"Auxiliary pose changed vertex count");
        for vector in &mut vectors {
            let source=*vector;
            ensure!(source[..3].iter().all(|v|v.is_finite()),"Nonfinite auxiliary direction");
            for i in 0..3 {vector[i]=self.mesh_to_model[i][0]*source[0]+self.mesh_to_model[i][1]*source[1]+self.mesh_to_model[i][2]*source[2];}
            ensure!(vector[..3].iter().all(|v|v.is_finite()),"Auxiliary model-space direction overflow");
            // Translation and the handedness lane are deliberately untouched.
        }
        Ok(vectors)
    }
    /// Exact FLOAT32 decoding for vector-deformation consumers. No conversion
    /// or guessed default is applied to unsupported component formats.
    pub fn float_vectors(&self,name:&str,index:u32)->Result<Vec<[f32;4]>> {
        let stream=self.streams.iter().find(|s|s.semantic==name&&s.index==index).context("Auxiliary vector semantic is absent")?;
        let count=((stream.format>>16)&255) as usize;
        ensure!((1..=4).contains(&count)&&((stream.format>>8)&255)==4&&(stream.format&255)==0x34+count as u32,"Auxiliary vector is not FLOAT32");
        let width=count*4;ensure!(stream.bytes.len()==self.vertices.checked_mul(width).context("Auxiliary vector size overflow")?,"Auxiliary vector byte count mismatch");
        let mut output=Vec::with_capacity(self.vertices);
        for bytes in stream.bytes.chunks_exact(width){
            let mut value=[0.,0.,0.,1.];
            for (i,word) in bytes.chunks_exact(4).enumerate(){value[i]=f32::from_bits(if self.little_endian {u32::from_le_bytes(word.try_into()?)}else{u32::from_be_bytes(word.try_into()?)});}
            output.push(value);
        }
        Ok(output)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_positions_do_not_round_trip_through_the_model_bake()->Result<()> {
        let source=Source {vertices:1,little_endian:true,mesh_to_model:[[1.,0.,0.,1e8],[0.,1.,0.,0.],[0.,0.,1.,0.],[0.,0.,0.,1.]],pose:None,streams:Arc::new(vec![Stream {semantic:"POSITION".into(),index:0,format:0x00030437,bytes:Arc::new([0.125f32,2.,3.].iter().flat_map(|v|v.to_le_bytes()).collect())}])};
        let native=source.mesh_vectors("POSITION",0)?;assert_eq!(native[0],[0.125,2.,3.,1.]);
        assert_ne!((0.125f32+1e8)-1e8,native[0][0]);Ok(())
    }
    #[derive(Debug)]
    struct PointPose;
    impl DirectionPose for PointPose {
        fn deform(&self,v:&mut [[f32;4]])->Result<()> {for v in v {v[0]=-v[0];}Ok(())}
        fn deform_positions(&self,v:&mut [[f32;4]])->Result<()> {for v in v {v[0]+=10.;}Ok(())}
    }
    #[test]
    fn point_pose_and_direction_pose_have_separate_translation_semantics()->Result<()> {
        let streams=[("POSITION",[1f32,2.,3.]),("NORMAL",[1.,0.,0.])].into_iter().map(|(semantic,value)|Stream {semantic:semantic.into(),index:0,format:0x00030437,bytes:Arc::new(value.iter().flat_map(|v|v.to_le_bytes()).collect())}).collect();
        let source=Source {vertices:1,little_endian:true,mesh_to_model:[[1.,0.,0.,100.],[0.,1.,0.,200.],[0.,0.,1.,300.],[0.,0.,0.,1.]],pose:Some(Arc::new(PointPose)),streams:Arc::new(streams)};
        assert_eq!(source.mesh_vectors("POSITION",0)?[0],[11.,2.,3.,1.]);assert_eq!(source.mesh_vectors("NORMAL",0)?[0],[-1.,0.,0.,1.]);Ok(())
    }

    #[derive(Debug)]
    struct Rotation(std::sync::atomic::AtomicUsize);
    impl DirectionPose for Rotation {
        fn deform(&self,vectors:&mut [[f32;4]])->Result<()> {self.0.fetch_add(1,std::sync::atomic::Ordering::Relaxed);for v in vectors {let x=v[0];v[0]=-v[1];v[1]=x;}Ok(())}
    }
    #[test]
    fn pose_is_lazy_and_precedes_node_direction_transform()->Result<()> {
        let pose=Arc::new(Rotation(std::sync::atomic::AtomicUsize::new(0)));
        let source=Source {vertices:1,little_endian:true,mesh_to_model:[[2.,0.,0.,50.],[0.,3.,0.,60.],[0.,0.,4.,70.],[0.,0.,0.,1.]],pose:Some(pose.clone()),streams:Arc::new(vec![Stream {semantic:"TANGENT".into(),index:0,format:0x00040438,bytes:Arc::new([1f32,2.,3.,-1.].iter().flat_map(|v|v.to_le_bytes()).collect())}])};
        assert_eq!(pose.0.load(std::sync::atomic::Ordering::Relaxed),0);
        assert_eq!(source.float_vectors("TANGENT",0)?[0],[1.,2.,3.,-1.]);
        assert_eq!(pose.0.load(std::sync::atomic::Ordering::Relaxed),0);
        assert_eq!(source.model_vectors("TANGENT",0)?[0],[-4.,3.,12.,-1.]);
        assert_eq!(pose.0.load(std::sync::atomic::Ordering::Relaxed),1);
        let clone=source.clone();assert!(Arc::ptr_eq(&source.streams,&clone.streams));Ok(())
    }
    #[test]
    fn modifier_data_is_not_indexed_as_vertex_input()->Result<()> {
        assert!(!is_auxiliary("MORPHWEIGHTS",0)?);assert!(!is_auxiliary("MORPH_POSITION",1)?);
        assert!(!is_auxiliary("BONE_PALETTE",0)?);assert!(is_auxiliary("TANGENT",0)?);
        assert!(is_auxiliary("UnknownControlData",0).is_err());Ok(())
    }
    #[test]
    fn source_mapping_preserves_interleaving_regions_and_repeated_vertices()->Result<()> {
        let bytes=(0..80u8).collect::<Vec<_>>();let layout=Layout {stride:20,offset:4,width:16,start:1,count:3};
        let output=gather(&bytes,&layout,&[2,0,2])?;
        assert_eq!(output,[&bytes[64..80],&bytes[24..40],&bytes[64..80]].concat());
        assert!(gather(&bytes[..79],&layout,&[2]).is_err());assert!(gather(&bytes,&layout,&[3]).is_err());Ok(())
    }
    #[test]
    fn float_vectors_preserve_handedness_and_big_endian_bits()->Result<()> {
        for little in [false,true]{
            let bits=[1f32.to_bits(),(-0f32).to_bits(),0x7fc12345,(-1f32).to_bits()];
            let bytes=bits.iter().flat_map(|v|if little {v.to_le_bytes()}else{v.to_be_bytes()}).collect();
            let source=Source {vertices:1,little_endian:little,mesh_to_model:[[0.;4];4],pose:None,streams:Arc::new(vec![Stream {semantic:"TANGENT".into(),index:0,format:0x00040438,bytes:Arc::new(bytes)}])};
            assert_eq!(source.float_vectors("TANGENT",0)?[0].map(f32::to_bits),bits);
        }Ok(())
    }
}
