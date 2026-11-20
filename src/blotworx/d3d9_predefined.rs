//! PC predefined shader bindings from explicit renderer caches. Matrices are
//! D3D row-vector matrices; GL camera conversion and inverse production remain
//! with the scene owner, never inferred from a shader parameter's name.
use anyhow::{ensure,Context,Result};
use std::collections::BTreeMap;
#[derive(Debug,Clone,Copy,PartialEq,Eq,PartialOrd,Ord)]
pub(super) enum Matrix {Projection,View,World,WorldView,ViewProjection,WorldViewProjection}
#[derive(Debug,Clone)]
pub(super) struct Cached {pub value:[f32;16],pub inverse:Option<[f32;16]>}
#[derive(Debug,Clone)]
pub(super) struct Cache {
    pub matrices:BTreeMap<Matrix,Cached>,pub eye_world:[f32;4],pub eye_direction_world:[f32;4],
    /// Native NiRect order: left, right, top, bottom; not pixel extent/reciprocal.
    pub viewport:[f32;4],
}
#[derive(Debug,PartialEq)]
pub(super) enum Value {Matrix([f32;16]),Vector([f32;4])}
pub(super) fn transpose(matrix:&[f32;16])->[f32;16] {std::array::from_fn(|i|matrix[(i%4)*4+i/4])}
/// Keep each multiply/add rounded separately and in the observed D3DX order.
/// Finite 64-bit Microsoft reference cases agree; original 32-bit numeric
/// equivalence, nonfinite values and all precision edges remain to verify.
pub(super) fn transform(vector:&[f32;4],matrix:&[f32;16])->[f32;4] {
    std::array::from_fn(|column|((vector[0]*matrix[column]+vector[1]*matrix[4+column])+vector[2]*matrix[8+column])+vector[3]*matrix[12+column])
}
pub(super) fn multiply(left:&[f32;16],right:&[f32;16])->[f32;16] {
    let rows:[[_;4];4]=std::array::from_fn(|r|transform(left[r*4..r*4+4].try_into().unwrap(),right));
    std::array::from_fn(|i|rows[i/4][i%4])
}
fn matrix_mapping(name:&str)->Option<(Matrix,bool,bool)> {
    let (base,inverse,transposed)=match name {
        _ if ["Proj","Projection"].iter().any(|alias|name.eq_ignore_ascii_case(alias))=>(Matrix::Projection,false,false),
        _ if ["InvProj","InvProjection"].iter().any(|alias|name.eq_ignore_ascii_case(alias))=>(Matrix::Projection,true,false),
        _ if ["ProjTranspose","ProjectionTranspose"].iter().any(|alias|name.eq_ignore_ascii_case(alias))=>(Matrix::Projection,false,true),
        _ if ["InvProjTranspose","InvProjectionTranspose","ProjInverseTranspose","ProjectionInverseTranspose"].iter().any(|alias|name.eq_ignore_ascii_case(alias))=>(Matrix::Projection,true,true),
        _ if ["View"].iter().any(|alias|name.eq_ignore_ascii_case(alias))=>(Matrix::View,false,false),_ if ["InvView","ViewInverse"].iter().any(|alias|name.eq_ignore_ascii_case(alias))=>(Matrix::View,true,false),
        _ if ["ViewTranspose"].iter().any(|alias|name.eq_ignore_ascii_case(alias))=>(Matrix::View,false,true),_ if ["InvViewTranspose","ViewInverseTranspose"].iter().any(|alias|name.eq_ignore_ascii_case(alias))=>(Matrix::View,true,true),
        _ if ["World"].iter().any(|alias|name.eq_ignore_ascii_case(alias))=>(Matrix::World,false,false),_ if ["InvWorld","WorldInverse"].iter().any(|alias|name.eq_ignore_ascii_case(alias))=>(Matrix::World,true,false),
        _ if ["WorldTranspose"].iter().any(|alias|name.eq_ignore_ascii_case(alias))=>(Matrix::World,false,true),_ if ["InvWorldTranspose","WorldInverseTranspose"].iter().any(|alias|name.eq_ignore_ascii_case(alias))=>(Matrix::World,true,true),
        _ if ["WorldView"].iter().any(|alias|name.eq_ignore_ascii_case(alias))=>(Matrix::WorldView,false,false),_ if ["InvWorldView","WorldViewInverse"].iter().any(|alias|name.eq_ignore_ascii_case(alias))=>(Matrix::WorldView,true,false),
        _ if ["WorldViewTranspose"].iter().any(|alias|name.eq_ignore_ascii_case(alias))=>(Matrix::WorldView,false,true),_ if ["InvWorldViewTranspose","WorldViewInverseTranspose"].iter().any(|alias|name.eq_ignore_ascii_case(alias))=>(Matrix::WorldView,true,true),
        _ if ["ViewProj","ViewProjection"].iter().any(|alias|name.eq_ignore_ascii_case(alias))=>(Matrix::ViewProjection,false,false),
        _ if ["InvViewProj","InvViewProjection","ViewProjInverse","ViewProjectionInverse"].iter().any(|alias|name.eq_ignore_ascii_case(alias))=>(Matrix::ViewProjection,true,false),
        _ if ["ViewProjTranspose","ViewProjectionTranspose"].iter().any(|alias|name.eq_ignore_ascii_case(alias))=>(Matrix::ViewProjection,false,true),
        _ if ["InvViewProjTranspose","InvViewProjectionTranspose","ViewProjInverseTranspose","ViewProjectionInverseTranspose"].iter().any(|alias|name.eq_ignore_ascii_case(alias))=>(Matrix::ViewProjection,true,true),
        _ if ["WorldViewProj","WorldViewProjection"].iter().any(|alias|name.eq_ignore_ascii_case(alias))=>(Matrix::WorldViewProjection,false,false),
        _ if ["InvWorldViewProj","InvWorldViewProjection","WorldViewProjInverse","WorldViewProjectionInverse"].iter().any(|alias|name.eq_ignore_ascii_case(alias))=>(Matrix::WorldViewProjection,true,false),
        _ if ["WorldViewProjTranspose","WorldViewProjectionTranspose"].iter().any(|alias|name.eq_ignore_ascii_case(alias))=>(Matrix::WorldViewProjection,false,true),
        _ if ["InvWorldViewProjTranspose","InvWorldViewProjectionTranspose","WorldViewProjInverseTranspose","WorldViewProjectionInverseTranspose"].iter().any(|alias|name.eq_ignore_ascii_case(alias))=>(Matrix::WorldViewProjection,true,true),
        _=>return None,
    };Some((base,inverse,transposed))
}
impl Cache {
    pub fn texgen_projection(&self,bound:[f32;3],precision:super::pc_float::Precision)->Result<Value> {
        let matrix=&self.matrices.get(&Matrix::ViewProjection).context("Texgen projection lacks camera matrix ownership")?.value;
        let projected=transform(&[bound[0],bound[1],bound[2],1.],matrix);
        let [scale,u,v]=super::pc_float::texgen_projection(bound,self.eye_world[..3].try_into().unwrap(),self.eye_direction_world[..3].try_into().unwrap(),projected,precision)?;
        Ok(Value::Matrix([scale,0.,0.,0.,0.,scale,0.,0.,0.,0.,0.,0.,u,v,0.,1.]))
    }
    pub fn compose(world:[f32;16],view:[f32;16],projection:[f32;16],eye_world:[f32;4],eye_direction_world:[f32;4],viewport:[f32;4])->Result<Self> {
        ensure!(world.iter().chain(&view).chain(&projection).chain(&eye_world).chain(&eye_direction_world).chain(&viewport).all(|v|v.is_finite()),"Nonfinite original renderer cache");
        let world_view=multiply(&world,&view);let view_projection=multiply(&view,&projection);let world_view_projection=multiply(&world_view,&projection);
        ensure!(world_view.iter().chain(&view_projection).chain(&world_view_projection).all(|v|v.is_finite()),"Original matrix product overflow");
        let matrices=[(Matrix::World,world),(Matrix::View,view),(Matrix::Projection,projection),(Matrix::WorldView,world_view),(Matrix::ViewProjection,view_projection),(Matrix::WorldViewProjection,world_view_projection)].into_iter().map(|(kind,value)|(kind,Cached {value,inverse:None})).collect();
        Ok(Self {matrices,eye_world,eye_direction_world,viewport})
    }
    pub fn set_inverse(&mut self,kind:Matrix,inverse:[f32;16])->Result<()> {
        ensure!(inverse.iter().all(|v|v.is_finite()),"Nonfinite original inverse cache");
        self.matrices.get_mut(&kind).context("Original matrix cache is absent")?.inverse=Some(inverse);Ok(())
    }
    /// None is an unhandled mapping for the next owner to resolve. It must not
    /// be interpreted as permission to retain a zero/default shader value.
    pub fn resolve(&self,name:&str)->Result<Option<Value>> {
        if let Some((kind,inverse,transposed))=matrix_mapping(name) {
            let cached=self.matrices.get(&kind).context("Original matrix cache is absent")?;
            let matrix=if inverse {cached.inverse.as_ref().context("Original inverse matrix owner is absent")?}else{&cached.value};
            return Ok(Some(Value::Matrix(if transposed {transpose(matrix)}else{*matrix})));
        }
        let vector=match name {
            _ if ["EyePos"].iter().any(|alias|name.eq_ignore_ascii_case(alias))=>self.eye_world,_ if ["EyeDir"].iter().any(|alias|name.eq_ignore_ascii_case(alias))=>self.eye_direction_world,
            _ if ["EyePosOS","EyeDirOS"].iter().any(|alias|name.eq_ignore_ascii_case(alias))=>{
                let inverse=self.matrices.get(&Matrix::World).and_then(|m|m.inverse.as_ref()).context("Original object-space eye mapping requires inverse-world ownership")?;
                transform(if name.eq_ignore_ascii_case("EyePosOS") {&self.eye_world}else{&self.eye_direction_world},inverse)
            },
            _ if ["ViewportDimensions"].iter().any(|alias|name.eq_ignore_ascii_case(alias))=>[(self.viewport[1]-self.viewport[0]).abs(),self.viewport[0],(self.viewport[2]-self.viewport[3]).abs(),self.viewport[2]],
            _=>return Ok(None),
        };
        ensure!(vector.iter().all(|v|v.is_finite()),"Original predefined vector overflow");Ok(Some(Value::Vector(vector)))
    }
}
impl Value {
    /// D3DX effect SetMatrix/GetValue uses row-major parameter storage even
    /// when the GPU CTAB packs a matrix by columns. CTAB packing is separate.
    pub fn words(&self,parameter:&super::d3d9_effect::Value)->Result<Vec<u32>> {
        ensure!(parameter.kind==3&&parameter.elements<=1,"Predefined binding requires its float/array adapter");
        let values=match self {
            Self::Matrix(matrix)=>{
                ensure!(parameter.class==2&&parameter.rows==parameter.columns&&matches!(parameter.rows,3|4),"Predefined matrix parameter requires its shape adapter");
                // SetMatrix takes a D3DXMATRIX even for float3x3; the effect
                // stores the leading rows/columns, excluding the padded lanes.
                (0..parameter.rows as usize).flat_map(|row|(0..parameter.columns as usize).map(move |column|matrix[row*4+column])).collect()
            },
            Self::Vector(vector)=>{
                ensure!(parameter.class==1&&parameter.rows==1&&(1..=4).contains(&parameter.columns),"Predefined vector parameter shape differs");vector[..parameter.columns as usize].to_vec()
            },
        };
        Ok(values.into_iter().map(f32::to_bits).collect())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_pc_texgen_projection_mapping()->Result<()> {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_texgen_projection.json"))?;
        assert_eq!(fixture["constants"]["epsilon_word"],0x358637bdu32);assert_eq!(fixture["constants"]["scale_word"],0x3d4ccccdu32);assert_eq!(fixture["constants"]["zero_word"],0);
        for row in fixture["cases"].as_array().unwrap() {
            let matrix=|name:&str|std::array::from_fn(|i|row[name][i].as_f64().unwrap() as f32);
            let point=|name:&str|std::array::from_fn(|i|row[name][i].as_f64().unwrap() as f32);
            let precision=match row["precision"].as_u64().unwrap(){0x7f=>super::super::pc_float::Precision::Single,0x27f=>super::super::pc_float::Precision::Double,0x37f=>super::super::pc_float::Precision::Extended,_=>unreachable!()};
            let eye:[f32;3]=point("eye");let direction:[f32;3]=point("direction");
            let cache=Cache::compose(identity(),matrix("view"),matrix("projection"),[eye[0],eye[1],eye[2],1.],[direction[0],direction[1],direction[2],0.],[0.,1.,1.,0.])?;
            let Value::Matrix(actual)=cache.texgen_projection(point("bound"),precision)? else{unreachable!()};
            assert_eq!(actual.map(f32::to_bits).to_vec(),row["words"].as_array().unwrap().iter().map(|v|v.as_u64().unwrap() as u32).collect::<Vec<_>>(),"{row}");
        }Ok(())
    }
    #[test]
    fn predefined_lookup_ignores_ascii_case_as_native_lookup_does()->Result<()> {
        let cache=Cache::compose(identity(),identity(),identity(),[1.,2.,3.,1.],[0.,0.,1.,0.],[0.,1.,1.,0.])?;
        assert_eq!(cache.resolve("wORLD")?,cache.resolve("World")?);
        assert_eq!(cache.resolve("viewportdimensions")?,cache.resolve("ViewportDimensions")?);
        assert_eq!(cache.resolve("eYEpOS")?,cache.resolve("EyePos")?);
        assert!(cache.resolve("time")?.is_none());Ok(())
    }
    fn identity()->[f32;16] {std::array::from_fn(|i|if i%5==0 {1.}else{0.})}
    #[test]
    fn missing_inverse_is_not_an_identity_substitute()->Result<()> {
        let cache=Cache::compose(identity(),identity(),identity(),[1.,2.,3.,1.],[0.,0.,1.,0.],[0.,1.,1.,0.])?;
        assert!(cache.resolve("EyePosOS").is_err());assert!(cache.resolve("WorldInverseTranspose").is_err());assert!(cache.resolve("TexTransformShader")?.is_none());Ok(())
    }
    #[test]
    fn viewport_mapping_preserves_origin_and_uses_absolute_spans()->Result<()> {
        let cache=Cache::compose(identity(),identity(),identity(),[0.;4],[0.;4],[0.875,0.125,0.25,0.75])?;
        assert_eq!(cache.resolve("ViewportDimensions")?,Some(Value::Vector([0.75,0.875,0.5,0.25])));Ok(())
    }
}
