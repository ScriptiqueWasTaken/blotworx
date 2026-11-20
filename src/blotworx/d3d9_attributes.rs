//! Mesh attribute to effect parameter storage. Type and shape conversions are
//! explicit, following the supporting Gamebryo D3DX parameter adapter. Original
//! PC setter-path validation remains required before scene-native admission.
use anyhow::{ensure,Result};
use crate::nif_shader_attributes::Attribute;
use super::d3d9_effect::Value;
/// Native predefined time reads this mesh-local float, not wall-clock time.
pub(super) fn shader_time(table:&crate::nif_shader_attributes::Table,parameter:&Value)->Result<Option<Vec<u32>>> {
    let Some(attribute)=table.get("NDL_UpdateTime") else{return Ok(None)};
    let Attribute::Float(value)=attribute else {anyhow::bail!("Original shader time is not float extra data")};
    Ok(Some(shader_time_value(f32::from_bits(*value),parameter)?))
}
pub(super) fn shader_time_value(time:f32,parameter:&Value)->Result<Vec<u32>> {
    ensure!(time.is_finite(),"Nonfinite shader time needs its native conversion adapter");
    words(&Attribute::Float(time.to_bits()),parameter)
}
pub(super) fn words(attribute:&Attribute,parameter:&Value)->Result<Vec<u32>> {
    ensure!(parameter.elements<=1,"Shader attribute arrays require their array adapter");
    let scalar=parameter.class==0&&parameter.rows==1&&parameter.columns==1;
    match attribute {
        Attribute::Boolean(value)=>{ensure!(scalar&&parameter.kind==1,"Boolean shader attribute type differs");Ok(vec![u32::from(*value!=0)])},
        Attribute::Integer(value)=>{ensure!(scalar&&parameter.kind==2,"Integer shader attribute type differs");Ok(vec![*value as u32])},
        Attribute::Float(value)=>{ensure!(scalar&&parameter.kind==3,"Float shader attribute type differs");Ok(vec![*value])},
        Attribute::Floats(values) if parameter.class==2=>{
            ensure!(parameter.kind==3&&parameter.rows==parameter.columns&&matches!(parameter.rows,3|4),"Matrix shader attribute requires its shape adapter");
            match (parameter.rows,values.len()) {
                (3,9)|(4,16)=>Ok(values.clone()),
                (3,16)=>Ok((0..3).flat_map(|row|(0..3).map(move |column|values[row*4+column])).collect()),
                _=>anyhow::bail!("Matrix shader attribute source size differs"),
            }
        },
        Attribute::Floats(values)=>{
            ensure!(parameter.kind==3&&parameter.class==1&&parameter.rows==1&&(1..=4).contains(&parameter.columns)&&(2..=4).contains(&values.len()),"Vector shader attribute shape differs");
            let mut vector=[0,0,0,1f32.to_bits()];vector[..values.len()].copy_from_slice(values);Ok(vector[..parameter.columns as usize].to_vec())
        },
        Attribute::Color(values)=>{
            ensure!(parameter.kind==3&&parameter.class==1&&parameter.rows==1&&(3..=4).contains(&parameter.columns),"Color shader attribute shape differs");Ok(values[..parameter.columns as usize].to_vec())
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shader_time_preserves_original_mesh_timestamp_and_missing_failure()->Result<()> {
        let reference:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_shader_time.json"))?;
        for case in reference["cases"].as_array().unwrap() {
            let mut table=crate::nif_shader_attributes::Table::new();
            if let Some(word)=case["input_word"].as_u64() {table.insert("NDL_UpdateTime".into(),Attribute::Float(word as u32));}
            let result=shader_time(&table,&parameter(3,0,1,1))?;
            assert_eq!(result.is_some(),case["success"].as_bool().unwrap());
            if let Some(result)=result {assert_eq!(result,[case["calls"][0]["word"].as_u64().unwrap() as u32]);}
        }
        let table=[("NDL_UpdateTime".into(),Attribute::Integer(0))].into_iter().collect();
        assert!(shader_time(&table,&parameter(3,0,1,1)).is_err());Ok(())
    }
    fn parameter(kind:u32,class:u32,rows:u32,columns:u32)->Value {
        Value {kind,class,rows,columns,elements:0,name:None,semantic:None,data:super::super::d3d9_effect::Data::Words(vec![])}
    }
    #[test]
    fn authored_types_and_vector_padding_are_explicit()->Result<()> {
        let vector=parameter(3,1,1,4);
        assert_eq!(words(&Attribute::Floats(vec![2f32.to_bits(),3f32.to_bits()]),&vector)?,[2f32.to_bits(),3f32.to_bits(),0,1f32.to_bits()]);
        assert_eq!(words(&Attribute::Color([1,2,3,4]),&parameter(3,1,1,3))?,[1,2,3]);
        assert_eq!(words(&Attribute::Boolean(255),&parameter(1,0,1,1))?,[1]);
        assert_eq!(words(&Attribute::Integer(-1),&parameter(2,0,1,1))?,[u32::MAX]);
        assert!(words(&Attribute::Float(0),&parameter(2,0,1,1)).is_err());
        assert!(words(&Attribute::Color([1,2,3,4]),&parameter(3,1,1,2)).is_err());
        assert_eq!(words(&Attribute::Floats(vec![0;9]),&parameter(3,2,3,3))?,vec![0;9]);
        assert_eq!(words(&Attribute::Floats((1..=16).collect()),&parameter(3,2,3,3))?,[1,2,3,5,6,7,9,10,11]);
        Ok(())
    }
}
