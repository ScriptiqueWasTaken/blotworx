//! Reflected InventoryComponent storage. Command clamping and notifications
//! belong to command receivers, not these raw property accessors.
use anyhow::{Context,Result,ensure};
impl super::assets::World {
    /// REL26b8a0 ->345330 ->344868: ignores the supplied owner and uses
    /// the primary player's ETicket setter, including its upper-bound rule.
    pub(super) fn reset_ticket_count_lua(&mut self,args:mlua::MultiValue)->mlua::Result<mlua::MultiValue> {
        if args.len()!=1 {self.report.warnings.push("ResetTicketCount: expected one argument".into());}
        let owner=self.player_entity;
        if self.entities.get(owner).is_some_and(|entity|!self.removed_entities.contains(&owner)
            && entity.components.iter().any(|c|c.class_name=="InventoryComponent")) {
            let _previous=self.inventory_float(owner,"ETicket",0).map_err(mlua::Error::external)?;
            let maximum=self.entities[owner].components.iter().find(|c|c.class_name=="InventoryComponent")
                .and_then(|c|c.properties.iter().find(|p|p.name=="ETicketMax"))
                .and_then(|p|p.value.as_f64()).map(|value|value as f32);
            let next=maximum.filter(|maximum|0.>*maximum).unwrap_or(0.);
            self.set_inventory_float(owner,"ETicket",0,next).map_err(mlua::Error::external)?;
        }
        Ok(mlua::MultiValue::new())
    }
    pub(super) fn inventory_float(&self,owner:usize,name:&str,index:i32)->Result<f32> {
        let field=self.entities[owner].components.iter().find(|c|c.class_name=="InventoryComponent")
            .and_then(|c|c.properties.iter().find(|p|p.name==name && p.class_name=="Float"))
            .context("Missing inventory float")?;
        if !field.value.is_array() && index==0 && owner==self.player_entity {
            if let Some(ammo)=&self.ammo.inventory {
                match name {
                    "Paint"=>return Ok(ammo.amounts[0]),"Thinner"=>return Ok(ammo.amounts[1]),
                    "PaintMax"=>return ammo.maxima[0].context("Missing live PaintMax"),
                    "ThinnerMax"=>return ammo.maxima[1].context("Missing live ThinnerMax"),_=>{}
                }
            }
        }
        let value=if field.value.is_array() {field.value.get(index as u32 as usize)} else if index==0 {Some(&field.value)} else {None};
        Ok(value.and_then(|v|v.as_f64()).context("Inventory float index missing; native failed-getter output is unverified")? as f32)
    }
    pub(super) fn set_inventory_float(&mut self,owner:usize,name:&str,index:i32,value:f32)->Result<()> {
        ensure!(value.is_finite(),"Nonfinite inventory property value is not admitted");
        let field=self.entities[owner].components.iter_mut().find(|c|c.class_name=="InventoryComponent")
            .and_then(|c|c.properties.iter_mut().find(|p|p.name==name && p.class_name=="Float"))
            .context("Missing inventory float")?;
        let array=field.value.is_array();
        let destination=if array {field.value.get_mut(index as u32 as usize)} else if index==0 {Some(&mut field.value)} else {None};
        let Some(destination)=destination else {return Ok(())};
        *destination=serde_json::json!(value);
        if !array && owner==self.player_entity {
            if let Some(ammo)=&mut self.ammo.inventory {
                match name {
                    "Paint"=>ammo.amounts[0]=value,"Thinner"=>ammo.amounts[1]=value,
                    "PaintMax"=>ammo.maxima[0]=Some(value),"ThinnerMax"=>ammo.maxima[1]=Some(value),_=>{}
                }
            }
        }
        Ok(())
    }
}

struct NewSketchFields<'a> {
    component:&'a mut epic_mickey_lib_rs::scene_file::Component,
    default_maximum:&'a str,
}
impl NewSketchFields<'_> {
    fn array(&mut self,name:&str)->Result<&mut Vec<serde_json::Value>> {
        self.component.properties.iter_mut().find(|p|p.name==name)
            .and_then(|p|p.value.as_array_mut()).with_context(||format!("Inventory {name} array missing"))
    }
}
impl super::inventory_commands::NewSketch for NewSketchFields<'_> {
    fn count(&mut self)->Result<u32> {Ok(u32::try_from(self.array("Sketch")?.len())?)}
    fn resize(&mut self,array:super::inventory_commands::SketchArray,count:u32)->Result<()> {
        use super::inventory_commands::SketchArray;
        let (name,value)=match array {SketchArray::Names=>("Sketch",serde_json::json!("")),SketchArray::Counts=>("SketchCount",serde_json::json!(0)),SketchArray::Maxima=>("SketchMax",serde_json::json!(0.))};
        self.array(name)?.resize(count as usize,value);Ok(())
    }
    fn default_maximum(&mut self)->Result<f32> {super::config_value::decimal_float(self.default_maximum).map_err(anyhow::Error::msg)}
    fn set_name(&mut self,index:u32,item:&str)->Result<()> {self.array("Sketch")?[index as usize]=serde_json::json!(item);Ok(())}
    fn set_count(&mut self,index:u32,count:i32)->Result<()> {self.array("SketchCount")?[index as usize]=serde_json::json!(count);Ok(())}
    fn set_maximum(&mut self,index:u32,maximum:f32)->Result<()> {self.array("SketchMax")?[index as usize]=serde_json::json!(maximum);Ok(())}
}
/// REL343a80..343c14 explicitly resizes all three arrays to names+1, then
/// stores the name, converted count and original SketchDefaultMax variable.
pub(super) fn add_new_sketch(component:&mut epic_mickey_lib_rs::scene_file::Component,item:&str,amount:f32,default_maximum:&str)->Result<()> {
    super::inventory_commands::add_new_sketch(item,amount,&mut NewSketchFields{component,default_maximum})?;
    Ok(())
}
