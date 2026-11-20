//! Native Damage Lua callback3223c0: target plus a physical stimulus amount.
//! Full event-f registry ordering and complete damage feedback remain pending.
use mlua::{Lua,MultiValue,Value};
impl super::assets::World {
    pub(super) fn damage_lua(&mut self,lua:&Lua,mut args:MultiValue)->mlua::Result<MultiValue> {
        if args.len()!=2 {
            self.report.warnings.push(format!("Damage: expected two arguments; retaining native stack-pop order"));
        }
        //278894 pops a Lua number into a float; nonnumeric values become zero.
        let amount=lua.coerce_number(args.pop_back().unwrap_or(Value::Nil))?.unwrap_or(0.) as f32;
        if !amount.is_finite() {return Err(mlua::Error::external("Nonfinite Damage amount requires native reference validation"));}
        let Some(target)=self.scripts.resolve_native_entity_argument(lua,args.pop_back().unwrap_or(Value::Nil))?
            .filter(|i|!self.removed_entities.contains(i)) else {return Ok(MultiValue::new())};
        //3224bc..322580 initializes type-f source=target, header=0, adds
        //stimulus2, and leaves attacker, alias, position and direction null/zero.
        let emission=super::projectile_stimulus::Emission {
            stimulus:2,amount,position:[0.;3],direction:[0.;3],target:Some(target as u64),
            owner:None,tag:String::new(),delivery:super::projectile_stimulus::Delivery::Target,
        };
        self.receive_combat(target,std::slice::from_ref(&emission)).map_err(mlua::Error::external)?;
        let limitation="Scripted Damage reaches Physical vulnerability, live health/invulnerability and known combat receivers. Native event-f registry ordering, source attribution, complete feedback and invalid-target dispatch parity remain incomplete.";
        if !self.report.unsupported.iter().any(|s|s==limitation) {self.report.unsupported.push(limitation.into());}
        Ok(MultiValue::new())
    }
}
