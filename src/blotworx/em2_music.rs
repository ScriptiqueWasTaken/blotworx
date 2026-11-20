//! GameState music replay metadata, PC97e170/97e250 and97d9a0/97da60.
//! Constructor97e743..97e754 initializes both interned strings to null.
//! Assigning these leaves does not post a sound or change a Wwise state.
#[derive(Default)]
pub struct Cache { event:Option<Vec<u8>>, target:Option<Vec<u8>> }
impl Cache {
    pub fn call(&mut self,lua:&mlua::Lua,name:&str,args:mlua::MultiValue)->mlua::Result<mlua::MultiValue> {
        use mlua::{FromLua,Value};
        let (setter,slot)=match name {
            "SetMusicEvent"=>(true,&mut self.event),"GetMusicEvent"=>(false,&mut self.event),
            "SetMusicTarget"=>(true,&mut self.target),"GetMusicTarget"=>(false,&mut self.target),
            _=>return Err(mlua::Error::external("Unknown music metadata operation")),
        };
        if setter {
            let value=args.back().cloned().unwrap_or(Value::Nil);
            *slot=match value {
                Value::Nil=>None,
                value=>{
                    let value=mlua::String::from_lua(value,lua)?;
                    Some(value.as_bytes().split(|b|*b==0).next().unwrap_or_default().to_vec())
                }
            };
            Ok(mlua::MultiValue::new())
        } else {
            Ok(mlua::MultiValue::from_vec(vec![match slot {Some(bytes)=>Value::String(lua.create_string(bytes)?),None=>Value::Nil}]))
        }
    }
    pub fn summary(&self)->serde_json::Value {
        fn display(value:&Option<Vec<u8>>)->serde_json::Value {
            match value {
                None=>serde_json::Value::Null,
                Some(bytes)=>match std::str::from_utf8(bytes) {Ok(text)=>serde_json::json!(text),Err(_)=>serde_json::json!({"bytes":bytes})},
            }
        }
        serde_json::json!({"event":display(&self.event),"target":display(&self.target),
            "scope":"verified game-state string leaves; save/reset and level-transition replay remain unconnected"})
    }
}

/// PC b239e0: two interned empty strings and an empty linked override list.
/// b23a90 appends; b23530 selects the tail; b23b10 removes the first match.
pub struct Overrides {current:Option<Vec<u8>>,default:Option<Vec<u8>>,items:Vec<Option<Vec<u8>>>}
impl Default for Overrides {
    fn default()->Self {Self {current:Some(vec![]),default:Some(vec![]),items:vec![]}}
}
impl Overrides {
    pub fn call(&mut self,lua:&mlua::Lua,name:&str,args:mlua::MultiValue,mut set_state:impl FnMut(u32,u32))->mlua::Result<mlua::MultiValue> {
        use mlua::{FromLua,Value};
        if name=="RemoveAllMusicStateOverride" {self.items.clear();} else {
            let value=match args.back().cloned().unwrap_or(Value::Nil) {
                Value::Nil=>None,
                value=>Some(mlua::String::from_lua(value,lua)?.as_bytes().split(|b|*b==0).next().unwrap_or_default().to_vec()),
            };
            match name {
                "AddMusicStateOverride"=>self.items.push(value),
                "ReplaceMusicStateOverride"=>{self.items.clear();self.items.push(value);},
                "RemoveMusicStateOverride"=>{if let Some(index)=self.items.iter().position(|v|v==&value){self.items.remove(index);}},
                _=>return Err(mlua::Error::external("Unknown music override operation")),
            }
        }
        let next=self.items.last().unwrap_or(&self.default);
        if next!=&self.current {
            // The nullable string overload refuses a null state ID, but the
            // music manager still commits its selected string (b23593..bb).
            if let Some(bytes)=next {
                let name=std::str::from_utf8(bytes).map_err(mlua::Error::external)?;
                if bytes.len()>=260 {return Err(mlua::Error::external("Music state exceeds the native name-conversion buffer"));}
                let state=super::em2_audio::event_id(name).map_err(mlua::Error::external)?;
                if state!=0 {set_state(0xe414d158,state);} // PC string overload rejects hash0.
            }
            self.current=next.clone();
        }
        Ok(mlua::MultiValue::new())
    }
    pub fn summary(&self)->serde_json::Value {
        let text=|v:&Option<Vec<u8>>|v.as_ref().map(|b|String::from_utf8_lossy(b).into_owned());
        serde_json::json!({"current":text(&self.current),"default":text(&self.default),"overrides":self.items.iter().map(text).collect::<Vec<_>>(),
            "scope":"verified override list and state-message production; default-state volume inputs, reset/save and state consumer remain unconnected"})
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_nullable_c_strings_keep_empty_bytes_case_and_top_argument()->mlua::Result<()> {
        let lua=mlua::Lua::new();let mut cache=Cache::default();
        for name in ["Event","Target"] {
            assert!(matches!(cache.call(&lua,&format!("GetMusic{name}"),mlua::MultiValue::new())?[0],mlua::Value::Nil));
            for bytes in [b"".as_slice(),b"Play_mx_Yensid_Tutorial",b"Value\0ignored",b"\xffraw"] {
                let args=mlua::MultiValue::from_vec(vec![mlua::Value::Integer(9),mlua::Value::String(lua.create_string(bytes)?)]);
                assert!(cache.call(&lua,&format!("SetMusic{name}"),args)?.is_empty());
                let result=cache.call(&lua,&format!("GetMusic{name}"),mlua::MultiValue::new())?;
                let mlua::Value::String(value)=&result[0] else {panic!("Expected stored string")};
                assert_eq!(value.as_bytes().as_ref(),bytes.split(|b|*b==0).next().unwrap());
            }
            cache.call(&lua,&format!("SetMusic{name}"),mlua::MultiValue::from_vec(vec![mlua::Value::Nil]))?;
            assert!(matches!(cache.call(&lua,&format!("GetMusic{name}"),mlua::MultiValue::new())?[0],mlua::Value::Nil));
        }
        Ok(())
    }
    #[test]
    fn overrides_select_tail_remove_oldest_duplicate_and_preserve_null_selection()->mlua::Result<()> {
        let lua=mlua::Lua::new();let mut overrides=Overrides::default();let mut emitted=Vec::new();
        let mut call=|state:&mut Overrides,name:&str,value:Option<&str>|->mlua::Result<()> {
            let value=match value {Some(s)=>mlua::Value::String(lua.create_string(s)?),None=>mlua::Value::Nil};
            state.call(&lua,name,mlua::MultiValue::from_vec(vec![value]),|group,value|emitted.push((group,value)))?;Ok(())
        };
        call(&mut overrides,"AddMusicStateOverride",Some("A"))?;
        call(&mut overrides,"AddMusicStateOverride",Some("B"))?;
        call(&mut overrides,"AddMusicStateOverride",Some("A"))?;
        call(&mut overrides,"RemoveMusicStateOverride",Some("A"))?;
        assert_eq!(overrides.items,[Some(b"B".to_vec()),Some(b"A".to_vec())]);
        call(&mut overrides,"RemoveMusicStateOverride",Some("A"))?;
        assert_eq!(overrides.current,Some(b"B".to_vec()));
        call(&mut overrides,"ReplaceMusicStateOverride",None)?;
        assert_eq!(overrides.current,None);
        call(&mut overrides,"RemoveAllMusicStateOverride",None)?;
        assert_eq!(overrides.current,Some(vec![]));
        drop(call);
        assert_eq!(emitted.len(),5); // A,B,A,B,empty. Null is committed without enqueue.
        assert!(emitted.iter().all(|(group,_)|*group==0xe414d158));
        assert_eq!(emitted.last().unwrap().1,0x811c9dc5);
        Ok(())
    }
}
