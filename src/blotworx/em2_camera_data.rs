//! PC f178c0/f39f80/f39eb0: camera script arguments and custom dataset writes.
//! Profile redirect/transition dispatch retains required collision and event
//! receivers. Never report a scalar write as an applied gameplay camera.
use anyhow::{ensure, Result};
use mlua::{MultiValue, Value};

pub(super) struct Request {
    pub target: Value,
    pub normal: [f32; 3], // Distance, PlacementPitch, LookPitch (native argument order).
    pub split: [f32; 3],
    pub duration: f32,
}
impl Request {
    pub fn parse_reset(lua:&mlua::Lua,args:MultiValue)->mlua::Result<Self> {
        let mut args=args.into_vec();let count=args.len();
        let duration=if count>1 {lua.coerce_number(args.pop().unwrap())?.unwrap_or(0.) as f32}else{0.5};
        if !duration.is_finite(){return Err(mlua::Error::external("Nonfinite PC camera reset duration is unsupported"));}
        let target=if count>0 {args.pop().unwrap()}else{Value::Nil};
        Ok(Self {target,normal:[0.;3],split:[0.;3],duration})
    }
    pub fn parse(lua: &mlua::Lua, args: MultiValue) -> mlua::Result<Self> {
        let mut args = args.into_vec();
        let count = args.len();
        let mut pop = || -> mlua::Result<f32> {
            // PC90bb90 always overwrites the destination; a present nil or
            // nonnumeric value becomes zero, rather than the optional default.
            let value = lua.coerce_number(args.pop().unwrap_or(Value::Nil))?.unwrap_or(0.) as f32;
            if !value.is_finite() {
                return Err(mlua::Error::external("Nonfinite PC camera parameter is unsupported"));
            }
            Ok(value)
        };
        let mut split = [f32::MAX; 3];
        for lane in (0..3).rev() {
            if count > 5 + lane { split[lane] = pop()?; }
        }
        let duration = if count > 4 { pop()? } else { 0.5 };
        let target = if count > 3 { args.pop().unwrap() } else { Value::Nil };
        let mut normal = [0.; 3];
        for lane in (0..3).rev() {
            if count > lane {
                normal[lane] = lua.coerce_number(args.pop().unwrap_or(Value::Nil))?.unwrap_or(0.) as f32;
            }
        }
        if normal.iter().any(|v| !v.is_finite()) {
            return Err(mlua::Error::external("Nonfinite PC camera parameter is unsupported"));
        }
        // PC compares against 3.000000043527274e36, not equality to FLT_MAX.
        // Values AT OR ABOVE the threshold inherit, including explicit values.
        let threshold = f32::from_bits(0x7c1071db);
        for lane in 0..3 { if split[lane] >= threshold { split[lane] = normal[lane]; } }
        Ok(Self { target, normal, split, duration })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub(super) struct Values {
    pub distance: f32,
    pub placement_pitch: f32,
    pub look_pitch: f32,
}
impl From<[f32; 3]> for Values {
    fn from(v: [f32; 3]) -> Self { Self { distance:v[0], placement_pitch:v[1], look_pitch:v[2] } }
}
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub(super) struct Pair { pub normal: Values, pub split_screen: Values }
#[derive(Debug, serde::Serialize)]
pub(super) struct Custom {
    pub enabled: bool,
    pub buffer: usize,
    pub values: [Option<Pair>; 2],
}
impl Default for Custom {
    fn default() -> Self { Self { enabled:false, buffer:0, values:[None; 2] } }
}
#[derive(Clone, Debug, serde::Serialize)]
pub(super) struct Transition {
    pub dataset: usize,
    pub buffer: usize,
    pub requested_name: &'static str,
    pub custom_name: String,
    pub custom_split_name: String,
    pub kind: u32,
    pub duration: f32,
    pub custom_enabled:bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize)]
pub(super) enum Stage { #[default] Check, Subscribe }
#[derive(Clone, Debug, serde::Serialize)]
pub(super) struct Deferred { pub name:String, pub kind:u32, pub duration:f32 }
pub(super) trait Receiver {
    fn check_dataset(&mut self,row:&super::em2_camera_profiles::Row)->Result<bool>;
    fn precision(&mut self)->Result<super::em2_camera_profiles::Precision>;
    fn subscribe_target(&mut self)->Result<()>;
}
#[derive(Debug, serde::Serialize)]
pub(super) struct Bank {
    pub selected: usize,
    pub custom: [Custom; 6],
    pub pending: Option<Transition>,
    pub writes: u64,
    pub resets:u64,
    pub profiles:Option<super::em2_camera_profiles::Catalog>,
    pub stage:Stage,
    pub deferred:Option<Deferred>,
    pub split_screen:bool,
    pub hold_buffer:Option<usize>,
}
impl Default for Bank {
    fn default() -> Self {
        // PCf3d76e/f3efb6: Default selected, custom flags and indices zero.
        Self { selected:1, custom:std::array::from_fn(|_|Custom::default()), pending:None, writes:0, resets:0,profiles:None,stage:Stage::Check,deferred:None,split_screen:false,hold_buffer:None }
    }
}
impl Bank {
    pub fn initialize_profiles(&mut self,settings:&std::collections::BTreeMap<String,String>)->Result<()> {
        ensure!(self.profiles.is_none()&&self.writes==0&&self.resets==0,"PC camera profiles already initialized or mutated");
        self.profiles=Some(super::em2_camera_profiles::Catalog::load(settings)?);Ok(())
    }
    /// Commit the native producer prefix, retaining an active transition for
    /// required collision/subscription receivers. Inactive datasets only
    /// update storage, exactly as f3a015..f3a034.
    pub fn write(&mut self, dataset: usize, request: &Request) -> Result<()> {
        let stem = match dataset {1=>"CustomDefault",2=>"CustomTravel",3=>"CustomCombat",_=>anyhow::bail!("PC custom camera dataset {dataset} has no registered script writer")};
        ensure!(self.pending.is_none(), "Unconsumed PC camera dataset transition");
        let data = &mut self.custom[dataset];
        data.enabled = true;
        data.buffer = (data.buffer + 1) & 1;
        data.values[data.buffer] = Some(Pair { normal:request.normal.into(), split_screen:request.split.into() });
        self.writes = self.writes.wrapping_add(1);
        if let Some(profiles)=self.profiles.as_mut() {
            let name=format!("{stem}{}",data.buffer);
            profiles.write_triplet(&name,request.normal)?;
            profiles.write_triplet(&format!("{name}_SplitScreen"),request.split)?;
        }
        if self.selected == dataset {
            let name = format!("{stem}{}",data.buffer);
            // f3a02a passes the nominal dataset name. f41b50 then resolves
            // custom/hold and split-screen redirects; do not claim that this
            // candidate is already the effective camera dataset.
            let requested_name=match dataset {1=>"Default",2=>"Travel",3=>"Combat",_=>unreachable!()};
            self.pending = Some(Transition {dataset,buffer:data.buffer,requested_name,custom_name:name.clone(),custom_split_name:format!("{name}_SplitScreen"),kind:1,duration:request.duration,custom_enabled:true});
        }
        Ok(())
    }
    pub fn reset(&mut self,dataset:usize,duration:f32)->Result<()> {
        let (name,stem)=match dataset {1=>("Default","CustomDefault"),2=>("Travel","CustomTravel"),3=>("Combat","CustomCombat"),_=>anyhow::bail!("PC custom camera reset dataset {dataset} has no registered script writer")};
        ensure!(self.pending.is_none(),"Unconsumed PC camera dataset transition");
        ensure!(duration.is_finite(),"Nonfinite PC camera reset duration");
        let data=&mut self.custom[dataset];data.enabled=false;self.resets=self.resets.wrapping_add(1);
        // f37440 reapplies even when already disabled. Cached custom values
        // and the alternate-buffer index survive reset.
        if self.selected==dataset {
            let custom_name=format!("{stem}{}",data.buffer);
            self.pending=Some(Transition {dataset,buffer:data.buffer,requested_name:name,custom_split_name:format!("{custom_name}_SplitScreen"),custom_name,kind:1,duration,custom_enabled:false});
        }
        Ok(())
    }
    /// f41b50 receiver ordering: hold/custom redirect, split redirect, collision
    /// admission, base selection, target-event subscription, deferred clear.
    pub fn dispatch(&mut self,receiver:&mut impl Receiver)->Result<bool> {
        let Some(request)=self.pending.as_ref() else{return Ok(false);};
        let profiles=self.profiles.as_mut().ok_or_else(||anyhow::anyhow!("PC camera profiles have not been constructed"))?;
        if self.stage==Stage::Check {
            let mut name=if let Some(buffer)=self.hold_buffer {
                ensure!(buffer<2,"Invalid native PC camera hold buffer");format!("CustomHold{buffer}")
            }else if self.custom[request.dataset].enabled {request.custom_name.clone()}
            else {request.requested_name.into()};
            if self.split_screen {name.push_str("_SplitScreen");}
            let row=profiles.find(&name)?;
            if !receiver.check_dataset(&profiles.rows[row])? {
                // A genuine negative native check defers; an unavailable
                // collision backend must return Err, never a fabricated false.
                self.deferred=Some(Deferred {name,kind:request.kind,duration:request.duration});
                self.pending=None;return Ok(false);
            }
            let precision=receiver.precision()?;
            profiles.select(&name,request.kind,request.duration,precision)?;
            self.stage=Stage::Subscribe;
        }
        // If subscription fails, preserve the applied selection and this stage
        // so retry cannot snapshot/reapply the same transition a second time.
        receiver.subscribe_target()?;
        self.deferred=None;self.pending=None;self.stage=Stage::Check;Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(lua:&mlua::Lua, code:&str)->Request {
        Request::parse(lua,lua.load(code).eval::<MultiValue>().unwrap()).unwrap()
    }
    #[test]
    fn native_optional_arguments_and_split_threshold() {
        let lua=mlua::Lua::new();
        let r=parse(&lua,"return 7,7,-1");
        assert_eq!(r.normal,[7.,7.,-1.]);assert_eq!(r.split,r.normal);assert_eq!(r.duration,0.5);assert!(matches!(r.target,Value::Nil));
        let r=parse(&lua,"return '8',9,-2,'player',nil,nil,2,3.1e36");
        assert_eq!(r.normal,[8.,9.,-2.]);assert_eq!(r.split,[0.,2.,-2.]);assert_eq!(r.duration,0.);
        assert!(matches!(r.target,Value::String(_)));
        let r=parse(&lua,"return 1,2,3,nil,0.25,3e36,-3.1e36,4");
        assert_eq!(r.split,[1.,-3.1e36,4.]);
        let r=parse(&lua,"return");assert_eq!(r.normal,[0.;3]);assert_eq!(r.split,[0.;3]);
    }
    #[test]
    fn native_tail_pop_ignores_prefix_and_invalid_numbers_become_zero() {
        let lua=mlua::Lua::new();
        let r=parse(&lua,"return 'ignored',1,2,3,nil,0.25,4,5,6");
        assert_eq!(r.normal,[1.,2.,3.]);assert_eq!(r.split,[4.,5.,6.]);assert_eq!(r.duration,0.25);
        let r=parse(&lua,"return false,{},'not a number'");assert_eq!(r.normal,[0.;3]);
        assert!(Request::parse(&lua,lua.load("return 1/0").eval().unwrap()).is_err());
    }
    #[test]
    fn native_double_buffers_preserve_inactive_and_retained_active_transition() -> Result<()> {
        let lua=mlua::Lua::new();let mut bank=Bank::default();
        let first=parse(&lua,"return 7,7,-1");
        bank.write(2,&first)?;assert!(bank.pending.is_none());assert_eq!(bank.custom[2].buffer,1);
        let second=parse(&lua,"return 8,9,-2,nil,1,4,5,6");
        bank.write(2,&second)?;assert_eq!(bank.custom[2].buffer,0);
        assert_eq!(bank.custom[2].values[1].unwrap().normal.distance,7.);
        assert_eq!(bank.custom[2].values[0].unwrap().split_screen.distance,4.);
        bank.write(1,&first)?;let p=bank.pending.as_ref().unwrap();
        assert_eq!(p.requested_name,"Default");assert_eq!(p.custom_name,"CustomDefault1");assert_eq!(p.custom_split_name,"CustomDefault1_SplitScreen");assert_eq!(p.kind,1);
        assert!(bank.write(1,&second).is_err());assert_eq!(bank.writes,3);
        assert_eq!(bank.custom[1].values[1].unwrap().normal.distance,7.);
        Ok(())
    }
    #[test]
    fn reset_keeps_custom_rows_and_reapplies_even_when_already_disabled()->Result<()> {
        let lua=mlua::Lua::new();let mut bank=Bank::default();let request=parse(&lua,"return 8,9,-2");
        bank.write(2,&request)?;let retained=bank.custom[2].values;
        bank.reset(2,0.25)?;assert!(!bank.custom[2].enabled);assert_eq!(bank.custom[2].buffer,1);
        assert_eq!(bank.custom[2].values,retained);assert!(bank.pending.is_none());
        bank.reset(1,0.)?;let p=bank.pending.as_ref().unwrap();assert!(!p.custom_enabled);assert_eq!(p.requested_name,"Default");assert_eq!(p.duration,0.);
        let reset=Request::parse_reset(&lua,lua.load("return 'unused','player',nil").eval()?)?;
        assert!(matches!(reset.target,Value::String(_)));assert_eq!(reset.duration,0.);
        assert_eq!(Request::parse_reset(&lua,MultiValue::new())?.duration,0.5);Ok(())
    }
    #[test]
    fn receiver_keeps_collision_failure_and_subscription_prefix_without_reapplying()->Result<()> {
        struct Host {admitted:bool,fail_subscription:bool,checks:usize,subscriptions:usize}
        impl Receiver for Host {
            fn check_dataset(&mut self,row:&super::super::em2_camera_profiles::Row)->Result<bool> {
                self.checks+=1;assert_eq!(row.name,"CustomDefault1");assert_eq!(row.values,[7.,-1.,7.,1.4,0.]);Ok(self.admitted)
            }
            fn precision(&mut self)->Result<super::super::em2_camera_profiles::Precision>{Ok(super::super::em2_camera_profiles::Precision::Extended)}
            fn subscribe_target(&mut self)->Result<()>{self.subscriptions+=1;ensure!(!self.fail_subscription,"Fixture target subscription unavailable");Ok(())}
        }
        let settings=super::super::em2_camera_profiles::FIELDS.iter().zip([5.5,-6.,14.,1.4,0.]).map(|(k,v)|(format!("FreeCamera_CameraData_Base_{k}"),v.to_string())).collect();
        let mut bank=Bank::default();bank.initialize_profiles(&settings)?;bank.write(1,&parse(&mlua::Lua::new(),"return 7,7,-1"))?;
        let mut host=Host {admitted:true,fail_subscription:true,checks:0,subscriptions:0};
        assert!(bank.dispatch(&mut host).is_err());assert_eq!(bank.stage,Stage::Subscribe);let selected=bank.profiles.as_ref().unwrap().selection;
        host.fail_subscription=false;assert!(bank.dispatch(&mut host)?);assert_eq!(host.checks,1);assert_eq!(host.subscriptions,2);
        assert_eq!(bank.profiles.as_ref().unwrap().selection,selected);assert!(bank.pending.is_none());
        Ok(())
    }
    #[test]
    fn rejected_hold_split_view_is_deferred_without_selecting_a_profile()->Result<()> {
        struct Host;
        impl Receiver for Host {
            fn check_dataset(&mut self,row:&super::super::em2_camera_profiles::Row)->Result<bool>{assert_eq!(row.name,"CustomHold1_SplitScreen");Ok(false)}
            fn precision(&mut self)->Result<super::super::em2_camera_profiles::Precision>{panic!("Rejected camera must not select a profile")}
            fn subscribe_target(&mut self)->Result<()>{panic!("Rejected camera must not subscribe")}
        }
        let settings=super::super::em2_camera_profiles::FIELDS.iter().map(|k|(format!("FreeCamera_CameraData_Base_{k}"),"1".into())).collect();
        let mut bank=Bank::default();bank.initialize_profiles(&settings)?;bank.split_screen=true;bank.hold_buffer=Some(1);
        bank.write(1,&parse(&mlua::Lua::new(),"return 7,7,-1"))?;
        let old=bank.profiles.as_ref().unwrap().selection;
        assert!(!bank.dispatch(&mut Host)?);assert!(bank.pending.is_none());assert_eq!(bank.profiles.as_ref().unwrap().selection,old);
        assert_eq!(bank.deferred.as_ref().unwrap().name,"CustomHold1_SplitScreen");Ok(())
    }
}
