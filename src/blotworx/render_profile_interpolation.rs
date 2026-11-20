//! PC scalar render-profile interpolation. Activation gating, vector/color
//! rounding, discrete fields and simulation scheduling remain separate.
use anyhow::{ensure,Result};
use super::pc_float::{self,Precision};
use super::render_profile_settings::{Settings,Value};
#[derive(Debug,Clone)]
pub(super) struct Scalar {pub current:f32,pub desired:f32,pub step:f32,precision:Precision}
impl Scalar {
    /// Caller must already have selected the native blend path and supplied
    /// the actual current value. This never invents an initial renderer state.
    pub fn begin(current:f32,desired:f32,seconds:f32,precision:Precision)->Result<Self> {
        let (_,step)=pc_float::profile_scalar_step(current,desired,seconds,precision)?;
        Ok(Self {current,desired,step,precision})
    }
    pub fn advance(&mut self)->Result<bool> {
        let (current,continuing)=pc_float::profile_scalar_advance(self.current,self.desired,self.step,self.precision)?;
        self.current=current;Ok(continuing)
    }
}
/// Native color setup stores the subtraction to a float before applying the
/// transition factor; scalar setup retains its x87 intermediate instead.
#[derive(Debug,Clone)]
pub(super) struct Color {pub channels:[Scalar;3]}
impl Color {
    pub fn begin(current:[f32;3],desired:[f32;3],seconds:f32,precision:Precision)->Result<Self> {
        let channel=|i:usize|->Result<Scalar> {
            let difference=desired[i]-current[i];
            let (_,step)=pc_float::profile_scalar_step(0.,difference,seconds,precision)?;
            Ok(Scalar {current:current[i],desired:desired[i],step,precision})
        };
        Ok(Self {channels:[channel(0)?,channel(1)?,channel(2)?]})
    }
    pub fn advance(&mut self)->Result<bool> {
        let mut continuing=false;
        for channel in &mut self.channels {continuing|=channel.advance()?;}
        Ok(continuing)
    }
}
/// Numeric settings only. Texture-path changes require the original retained
/// LUT/film-grain resource lifecycle and are explicitly rejected here.
#[derive(Debug,Clone)]
pub(super) struct Numeric {
    pub current:Settings,
    fields:Vec<(usize,Vec<Scalar>)>,
    immediate:Vec<(usize,Value)>,
    pub active:bool,
    pub precision:Precision,
}
pub(super) fn blend_gate(previous:bool,seconds:f32)->Result<bool> {
    ensure!(seconds.is_finite(),"Nonfinite render-profile transition time");
    Ok(previous&&seconds>=f32::from_bits(0x3d088889))
}
impl Numeric {
    pub fn begin(current:&Settings,desired:&Settings,seconds:f32,precision:Precision)->Result<Self> {
        let mut result=Self {current:current.clone(),fields:Vec::new(),immediate:Vec::new(),active:true,precision};
        for (index,((name,current),(_,desired))) in current.entries().zip(desired.entries()).enumerate() {
            if matches!(name,"Range"|"Priority"|"InterpolateTime"){continue;}
            let rounded=|current:f32,desired:f32|->Result<Scalar>{
                let (_,step)=pc_float::profile_scalar_step(0.,desired-current,seconds,precision)?;
                Ok(Scalar {current,desired,step,precision})
            };
            let fields=match (current,desired) {
                (Value::Float(_),Value::Float(_)) if name=="ColorSwapWeight"=>{result.immediate.push((index,desired.clone()));continue;},
                (Value::Float(a),Value::Float(b))=>vec![Scalar::begin(*a,*b,seconds,precision)?],
                (Value::Color(a),Value::Color(b))|(Value::Point3(a),Value::Point3(b))=>a.iter().zip(b).map(|(a,b)|rounded(*a,*b)).collect::<Result<_>>()?,
                (Value::Point2(a),Value::Point2(b))=>a.iter().zip(b).map(|(a,b)|rounded(*a,*b)).collect::<Result<_>>()?,
                (Value::Int(_),Value::Int(_)) if name=="FogType"=>{result.current.values[index]=desired.clone();continue;},
                // Native setup writes desired RNG integers into the step
                // structure, but its numeric update does not apply them.
                (Value::Int(_),Value::Int(_)) if matches!(name,"FilmGrainBlackNoiseRNGTime"|"FilmGrainWhiteNoiseRNGTime")=>continue,
                (Value::Text(a),Value::Text(b))=>{ensure!(a==b,"Render-profile texture transition {name} requires retained resource ownership");continue;},
                _=>anyhow::bail!("Unsupported render-profile transition field {name}"),
            };
            result.fields.push((index,fields));
        }Ok(result)
    }
    pub fn advance(&mut self)->Result<bool> {
        if !self.active{return Ok(false);}
        for (index,value) in self.immediate.drain(..){self.current.values[index]=value;}
        let mut active=false;
        for (index,fields) in &mut self.fields {
            for field in fields.iter_mut(){active|=field.advance()?;}
            match &mut self.current.values[*index] {
                Value::Float(value)=>*value=fields[0].current,
                Value::Color(values)|Value::Point3(values)=>{for (value,field) in values.iter_mut().zip(fields.iter()){*value=field.current;}},
                Value::Point2(values)=>{for (value,field) in values.iter_mut().zip(fields.iter()){*value=field.current;}},
                _=>unreachable!("Numeric plan only contains validated numeric settings"),
            }
        }
        self.active=active;Ok(active)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_pc_gate_and_all_numeric_settings()->Result<()> {
        let gate:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_render_profile_gate.json"))?;
        for row in gate["cases"].as_array().unwrap(){assert_eq!(blend_gate(row["previous"].as_bool().unwrap(),f32::from_bits(row["seconds"].as_u64().unwrap() as u32))?,row["blend"].as_bool().unwrap(),"{row}");}
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_render_profile_kernel.json"))?;
        let layout:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_render_profile_layout.json"))?;
        let getters=layout["getters"].as_array().unwrap();
        let decode=|words:&serde_json::Value|->Result<Settings>{
            let mut settings=Settings::read(|_|None,true)?;
            for getter in getters {
                let Some(offset)=getter["destination"].as_u64() else{continue;};
                let i=offset as usize/4;let f=|lane|f32::from_bits(words[i+lane].as_u64().unwrap() as u32);
                let value=&mut settings.values[getter["index"].as_u64().unwrap() as usize];
                match value {
                    Value::Float(v)=>*v=f(0),Value::Int(v)=>*v=words[i].as_u64().unwrap() as u32 as i32,
                    Value::Color(v)|Value::Point3(v)=>*v=std::array::from_fn(f),Value::Point2(v)=>*v=std::array::from_fn(f),Value::Text(_)=>assert_eq!(words[i],0),
                }
            }Ok(settings)
        };
        for row in fixture["cases"].as_array().unwrap() {
            let precision=match row["precision"].as_u64().unwrap(){0x7f=>Precision::Single,0x27f=>Precision::Double,0x37f=>Precision::Extended,_=>unreachable!()};
            let mut transition=Numeric::begin(&decode(&row["initial"])?,&decode(&row["desired"])?,f32::from_bits(row["seconds"].as_u64().unwrap() as u32),precision)?;
            assert!(transition.current.same_bits(&decode(&row["prepared"])?));
            let mut frame=0;
            for snapshot in row["frames"].as_array().unwrap(){
                while frame<snapshot["frame"].as_u64().unwrap(){transition.advance()?;frame+=1;}
                let expected=decode(&snapshot["current"])?;
                for ((name,actual),(_,expected)) in transition.current.entries().zip(expected.entries()) {
                    assert_eq!(actual.json(),expected.json(),"{name}, frame {frame}, precision {precision:?}");
                }
            }
        }Ok(())
    }
    #[test]
    fn original_pc_color_kernel_frames()->Result<()> {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_render_profile_kernel.json"))?;
        let layout:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_render_profile_layout.json"))?;
        for row in fixture["cases"].as_array().unwrap() {
            let precision=match row["precision"].as_u64().unwrap(){0x7f=>Precision::Single,0x27f=>Precision::Double,0x37f=>Precision::Extended,_=>unreachable!()};
            for getter in layout["getters"].as_array().unwrap().iter().filter(|v|v["slot"].as_u64()==Some(42)) {
                let offset=getter["destination"].as_u64().unwrap() as usize/4;
                let values=|name:&str|std::array::from_fn(|i|f32::from_bits(row[name][offset+i].as_u64().unwrap() as u32));
                let mut color=Color::begin(values("initial"),values("desired"),f32::from_bits(row["seconds"].as_u64().unwrap() as u32),precision)?;
                for i in 0..3 {assert_eq!(color.channels[i].step.to_bits(),row["steps"][offset+i].as_u64().unwrap() as u32,"{getter}");}
                let mut frame=0;
                for snapshot in row["frames"].as_array().unwrap() {
                    while frame<snapshot["frame"].as_u64().unwrap() {color.advance()?;frame+=1;}
                    for i in 0..3 {assert_eq!(color.channels[i].current.to_bits(),snapshot["current"][offset+i].as_u64().unwrap() as u32,"{getter}, frame {frame}");}
                }
            }
        }Ok(())
    }
    #[test]
    fn original_pc_scalar_steps_and_advances()->Result<()> {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_render_profile_scalar.json"))?;
        let precision=|row:&serde_json::Value|match row["precision"].as_u64().unwrap(){0x7f=>Precision::Single,0x27f=>Precision::Double,0x37f=>Precision::Extended,_=>unreachable!()};
        let value=|row:&serde_json::Value,name:&str|f32::from_bits(row[name].as_u64().unwrap() as u32);
        for row in fixture["steps"].as_array().unwrap() {
            let (factor,step)=pc_float::profile_scalar_step(value(row,"current"),value(row,"desired"),value(row,"seconds"),precision(row))?;
            assert_eq!(factor.to_bits(),row["factor"].as_u64().unwrap() as u32,"{row}");
            assert_eq!(step.to_bits(),row["step"].as_u64().unwrap() as u32,"{row}");
            assert_eq!(Scalar::begin(value(row,"current"),value(row,"desired"),value(row,"seconds"),precision(row))?.step.to_bits(),step.to_bits());
        }
        for row in fixture["advances"].as_array().unwrap() {
            let mut scalar=Scalar {current:value(row,"current"),desired:value(row,"desired"),step:value(row,"step"),precision:precision(row)};
            let continuing=scalar.advance()?;
            assert_eq!(scalar.current.to_bits(),row["result"].as_u64().unwrap() as u32,"{row}");
            assert_eq!(continuing,row["continuing"].as_bool().unwrap(),"{row}");
        }Ok(())
    }
}
