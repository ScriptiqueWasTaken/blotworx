//! PC GameCameraPass datasets, b24b20/b248b0/b24900/b24d90/b24a90.
//! PC stores five independent floats per row, rather than EM1 property pointers.
//! Collision admission and gameplay camera generation remain separate receivers.
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;

pub(super) const FIELDS: [&str; 5] = ["Distance", "LookPitch", "PlacementPitch", "TargetHeightOffset", "TargetRadius"];
#[derive(Clone, Debug, serde::Serialize)]
pub(super) struct Row { pub name:String, pub values:[f32;5] }
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize)]
pub(super) struct Selection {
    pub current:usize, pub previous:Option<usize>, pub weight:f32,
    pub duration:f32, pub elapsed:f32, pub immediate:bool,
}
pub(super) use super::pc_float::Precision;
use super::pc_float::{blend,clock};

#[derive(Debug)]
pub(super) struct Catalog { pub rows:Vec<Row>, pub selection:Selection }
impl serde::Serialize for Catalog {
    fn serialize<S:serde::Serializer>(&self,serializer:S)->std::result::Result<S::Ok,S::Error> {
        use serde::ser::SerializeStruct;
        // Do not copy all thirty configuration rows into every frame report.
        let mut view=serializer.serialize_struct("PcCameraProfiles",4)?;
        view.serialize_field("row_count",&self.rows.len())?;
        view.serialize_field("selection",&self.selection)?;
        view.serialize_field("current",&self.rows.get(self.selection.current))?;
        view.serialize_field("previous",&self.selection.previous.and_then(|i|self.rows.get(i)))?;
        view.end()
    }
}
impl Catalog {
    pub fn load(settings:&BTreeMap<String,String>)->Result<Self> {
        let mut base=[0.;5];
        for (lane,field) in FIELDS.iter().enumerate() {
            let key=format!("FreeCamera_CameraData_Base_{field}");
            base[lane]=number(settings.get(&key).with_context(||format!("PC camera base scalar {key} is absent; native uninitialized fallback is unsupported"))?)?;
        }
        let mut result=Self {rows:vec![Row {name:"Base".into(),values:base}],selection:Selection::default()};
        result.derive("Intermediate",0,settings)?;
        for name in ["LockOn","Default","Travel","Combat","SnapShot","HoverWatch"] {
            let parent=result.derive(name,0,settings)?;
            result.derive(&format!("{name}_SplitScreen"),parent,settings)?;
        }
        for buffer in 0..2 {
            for stem in ["CustomDefault","CustomCombat","CustomTravel","CustomHold"] {
                let name=format!("{stem}{buffer}");
                result.derive(&name,0,settings)?;
                // PC f3f106 passes null base: custom split rows inherit Base,
                // independently of the corresponding ordinary custom row.
                result.derive(&format!("{name}_SplitScreen"),0,settings)?;
            }
        }
        Ok(result)
    }
    fn derive(&mut self,name:&str,parent:usize,settings:&BTreeMap<String,String>)->Result<usize> {
        let mut values=self.rows.get(parent).context("PC camera base row absent")?.values;
        for (lane,field) in FIELDS.iter().enumerate() {
            if let Some(text)=settings.get(&format!("FreeCamera_CameraData_{name}_{field}")) {values[lane]=number(text)?;}
        }
        let index=self.rows.len();self.rows.push(Row {name:name.into(),values});Ok(index)
    }
    pub fn find(&self,name:&str)->Result<usize> {
        self.rows.iter().position(|r|r.name==name).with_context(||format!("PC camera dataset {name} is absent"))
    }
    pub fn write_triplet(&mut self,name:&str,values:[f32;3])->Result<()> {
        ensure!(values.iter().all(|v|v.is_finite()),"Nonfinite PC custom camera values");
        let index=self.find(name)?;let row=&mut self.rows[index].values;
        row[0]=values[0];row[2]=values[1];row[1]=values[2];Ok(())
    }
    pub fn evaluate(&self,lane:usize,precision:Precision)->Result<f32> {
        ensure!(lane<5,"Invalid PC camera attribute lane");
        let s=self.selection;let incoming=self.rows.get(s.current).context("PC current camera row absent")?.values[lane];
        if let Some(previous)=s.previous {
            let outgoing=self.rows.get(previous).context("PC previous camera row absent")?.values[lane];
            blend(outgoing,incoming,s.weight,precision)
        }else {Ok(incoming)}
    }
    fn snapshot(&mut self,precision:Precision)->Result<usize> {
        let index=self.find("Intermediate")?;
        // Native b24900 reads/writes one lane at a time, including when the
        // intermediate row is itself an endpoint of an interrupted blend.
        for lane in 0..5 {self.rows[index].values[lane]=self.evaluate(lane,precision)?;}
        Ok(index)
    }
    /// Base transition only. Callers must first perform f41b50 name redirects
    /// and CheckDataSet; this is not a bypass for those collision predicates.
    pub fn select(&mut self,name:&str,kind:u32,duration:f32,precision:Precision)->Result<()> {
        ensure!(duration.is_finite(),"Nonfinite PC camera transition duration");
        let target=self.find(name)?;let mut kind=kind;
        if kind==1 && duration==0. {kind=0;} else if kind==1 {
            self.selection.duration=duration;self.selection.elapsed=0.;
        }else {self.selection.duration=-1.;}
        if kind==1||kind==2 {
            self.selection.previous=Some(if self.selection.previous.is_none()&&target!=self.selection.current {
                self.selection.current
            }else {self.snapshot(precision)?});
        }else {self.selection.previous=None;}
        self.selection.current=target;self.selection.immediate=kind==0;
        self.selection.weight=if kind==0 {1.}else{0.};Ok(())
    }
    pub fn advance(&mut self,delta:f32,precision:Precision)->Result<()> {
        ensure!(delta.is_finite()&&delta>=0.,"Invalid PC camera game clock delta");
        if self.selection.duration<=0. {return Ok(());}
        let (elapsed,weight)=clock(self.selection.elapsed,delta,self.selection.duration,precision)?;
        self.selection.elapsed=elapsed;self.selection.weight=weight;
        if weight>=1. {self.selection.previous=None;self.selection.duration=-1.;}
        Ok(())
    }
}
fn number(text:&str)->Result<f32> {
    let value=text.trim().parse::<f64>().context("Unsupported PC camera numeric configuration")? as f32;
    ensure!(value.is_finite(),"Nonfinite PC camera configuration");Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pc_rows_copy_values_and_custom_split_inherits_base_independently()->Result<()> {
        let mut settings=BTreeMap::new();
        for (field,value) in FIELDS.iter().zip([5.5,-6.,14.,1.4,0.]) {settings.insert(format!("FreeCamera_CameraData_Base_{field}"),value.to_string());}
        settings.insert("FreeCamera_CameraData_Default_TargetHeightOffset".into(),"4.4".into());
        settings.insert("FreeCamera_CameraData_CustomDefault0_TargetHeightOffset".into(),"8".into());
        let mut c=Catalog::load(&settings)?;assert_eq!(c.rows.len(),30);
        assert_eq!(c.rows[c.find("Default_SplitScreen")?].values[3],4.4);
        assert_eq!(c.rows[c.find("CustomDefault0")?].values[3],8.);
        assert_eq!(c.rows[c.find("CustomDefault0_SplitScreen")?].values[3],1.4);
        c.write_triplet("CustomDefault0",[7.,7.,-1.])?;
        assert_eq!(c.rows[c.find("CustomDefault0")?].values,[7.,-1.,7.,8.,0.]);Ok(())
    }
    #[test]
    #[cfg(target_arch="x86_64")]
    fn pc_blend_preserves_host_fp_state_and_native_zero_duration_fields()->Result<()> {
        let mut c=Catalog {rows:vec![Row {name:"Base".into(),values:[0.;5]},Row {name:"Intermediate".into(),values:[20.;5]},Row {name:"Next".into(),values:[10.;5]}],selection:Selection {current:0,previous:Some(2),weight:0.75,duration:17.,elapsed:4.,immediate:false}};
        fn environment()->[u8;32] {
            #[repr(align(16))] struct Fp([u8;512]);let mut fp=Fp([0;512]);
            unsafe{std::arch::asm!("fxsave64 [{p}]",p=in(reg) fp.0.as_mut_ptr(),options(nostack,preserves_flags));}
            fp.0[..32].try_into().unwrap()
        }
        let before=environment();
        for precision in [Precision::Single,Precision::Double,Precision::Extended] {assert_eq!(c.evaluate(0,precision)?,2.5);}
        assert_eq!(before,environment());
        c.select("Next",1,0.,Precision::Extended)?;
        assert_eq!(c.selection,Selection {current:2,previous:None,weight:1.,duration:17.,elapsed:4.,immediate:true});Ok(())
    }
    #[test]
    #[cfg(target_arch="x86_64")]
    fn pc_clock_uses_smoothstep_and_clears_completed_outgoing_row()->Result<()> {
        let mut c=Catalog {rows:vec![],selection:Selection {duration:1.,previous:Some(0),..Default::default()}};
        c.advance(0.25,Precision::Extended)?;assert_eq!(c.selection.weight,0.15625);
        c.advance(0.75,Precision::Extended)?;assert_eq!(c.selection.weight,1.);assert_eq!(c.selection.previous,None);assert_eq!(c.selection.duration,-1.);Ok(())
    }
}
