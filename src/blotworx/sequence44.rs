//! Version44 Apprentice records. PC JPS primitives retain padded-word and
//! nullable-string semantics; event/condition tags are not EM1's tag mapping.
use anyhow::{ensure,Context,Result};
use serde_json::{json,Value};
#[derive(Debug,serde::Serialize)]
pub(super) struct Condition {pub tag:u16,pub fields:Value}
#[derive(Debug,serde::Serialize)]
pub(super) struct Event {pub tag:u16,pub label:Option<String>,pub fields:Value}
#[derive(Debug,serde::Serialize)]
pub(super) struct Sequence {
    pub name:String,pub role:Option<String>,pub region:Option<String>,pub flags:u16,pub priority:u16,
    pub events:Vec<Event>,pub condition:Condition,
}
struct Reader<'a> {bytes:&'a [u8],at:usize}
impl Reader<'_> {
    fn take(&mut self,n:usize)->Result<&[u8]> {
        let end=self.at.checked_add(n).context("Apprentice record overflow")?;
        let value=self.bytes.get(self.at..end).with_context(||format!("Truncated Apprentice record at {:#x}",self.at))?;
        self.at=end;Ok(value)
    }
    fn word(&mut self)->Result<u32>{Ok(u32::from_le_bytes(self.take(4)?.try_into()?))}
    fn short(&mut self)->Result<u16>{Ok(self.word()? as u16)}
    // PC a4be10 reads the first byte but advances four bytes. Padding is not
    // part of truth evaluation (in particular, this is not word == -1).
    fn boolean(&mut self)->Result<bool>{Ok(self.take(4)?[0]!=0)}
    fn float(&mut self)->Result<f32>{let n=f32::from_bits(self.word()?);ensure!(n.is_finite(),"Nonfinite Apprentice float");Ok(n)}
    fn count(&mut self,minimum:usize)->Result<usize> {
        let count=self.word()? as usize;
        ensure!(count<=(self.bytes.len()-self.at)/minimum,"Apprentice count exceeds remaining records at {:#x}",self.at-4);Ok(count)
    }
    fn string(&mut self)->Result<Option<String>> {
        let start=self.at;let header=self.take(2)?;let span=header[0] as usize;let length=header[1] as usize;
        ensure!(span>=4 && span%4==0 && length<=span-2,"Invalid Apprentice string span at {start:#x}");
        let data=self.take(span-2)?;
        if length==0 {return Ok(None);}
        let text=&data[..length];ensure!(text.last()==Some(&0) && !text[..length-1].contains(&0),"Invalid Apprentice string terminator at {start:#x}");
        Ok(Some(std::str::from_utf8(&text[..length-1])?.to_owned()))
    }
    fn strings(&mut self,n:usize)->Result<Vec<Option<String>>>{(0..n).map(|_|self.string()).collect()}
    fn pairs(&mut self)->Result<Value> {
        let count=self.count(8)?;let mut values=Vec::new();for _ in 0..count {values.push(json!({"name":self.string()?,"value":self.short()? as i16}));}Ok(json!(values))
    }
    fn dialog(&mut self)->Result<Value> {
        let mut strings=self.strings(5)?;let flag=self.boolean()?;strings.extend(self.strings(2)?);
        Ok(json!({"strings":strings,"flag":flag}))
    }
    fn condition(&mut self)->Result<Condition> {
        let tag=self.short()?;
        let fields=match tag {
            0|1=>{let count=self.count(4)?;let mut children=Vec::new();for _ in 0..count {children.push(self.condition()?);}json!({"children":children})},
            2|4|5=>json!({"name":self.string()?,"value":self.short()? as i16,"comparison":self.short()?}),
            3=>json!({"mood":self.string()?}),
            6=>json!({"quest":self.string()?,"objective":self.string()?,"state":self.string()?}),
            _=>anyhow::bail!("Unknown Apprentice44 condition {tag} at {:#x}",self.at-4),
        };Ok(Condition {tag,fields})
    }
    fn sell_item(&mut self)->Result<Value> {
        let strings=self.strings(2)?;let kind=self.short()?;let flags=[self.boolean()?,self.boolean()?];
        let integers=[self.word()? as i32,self.word()? as i32];let shorts=[self.short()?,self.short()?];
        let flag=self.boolean()?;let changes=self.pairs()?;let lua=self.string()?;let condition=self.condition()?;
        Ok(json!({"strings":strings,"kind":kind,"flags":flags,"integers":integers,"shorts":shorts,"flag":flag,"changes":changes,"lua":lua,"condition":condition}))
    }
    fn event(&mut self)->Result<Event> {
        let tag=self.short()?;let label=self.string()?;
        let fields=match tag {
            0=>json!({"dialog":self.dialog()?}),
            1=>{let mode=self.short()?;let count=self.count(32)?;let mut dialogs=Vec::new();for _ in 0..count {dialogs.push(self.dialog()?);}json!({"mode":mode,"dialogs":dialogs})},
            2=>{let dialog=self.dialog()?;let key=self.string()?;let count=self.count(8)?;let mut choices=Vec::new();for _ in 0..count {choices.push(self.strings(2)?);}json!({"dialog":dialog,"key":key,"choices":choices,"condition":self.condition()?})},
            3=>json!({"strings":self.strings(2)?,"seconds":self.float()?,"flags":[self.boolean()?,self.boolean()?]}),
            4=>{let count=self.short()? as usize;ensure!(count<=(self.bytes.len()-self.at)/32,"Branch count exceeds remaining sequence records");let mut sequences=Vec::new();for _ in 0..count {sequences.push(self.sequence()?);}json!({"sequences":sequences})},
            5=>json!({"strings":self.strings(2)?,"flag":self.boolean()?,"condition":self.condition()?}),
            6=>json!({"operation":self.short()?,"item":self.string()?,"quantity":self.word()? as i32,"flag":self.boolean()?}),
            7=>json!({"lua":self.string()?}),
            8=>json!({"mode":self.short()?}),
            9=>json!({"flag":self.boolean()?,"changes":self.pairs()?}),
            10=>json!({"seconds":self.float()?}),
            11=>{let key=self.string()?;let kind=self.short()?;let count=self.count(56)?;let mut items=Vec::new();for _ in 0..count {items.push(self.sell_item()?);}json!({"key":key,"kind":kind,"items":items})},
            12=>json!({"sequence":self.string()?,"flag":self.boolean()?}),
            15=>json!({"strings":self.strings(3)?,"flag":self.boolean()?}),
            16=>json!({"text":self.string()?,"seconds":self.float()?}),
            _=>anyhow::bail!("Unconstructed Apprentice44 event {tag} at {:#x}",self.at-4),
        };Ok(Event {tag,label,fields})
    }
    fn sequence(&mut self)->Result<Sequence> {
        let name=self.string()?.context("Unnamed Apprentice sequence")?;let role=self.string()?;let region=self.string()?;
        let flags=self.short()?;let priority=self.short()?;let count=self.count(8)?;
        let mut events=Vec::new();for _ in 0..count {events.push(self.event()?);}
        Ok(Sequence {name,role,region,flags,priority,events,condition:self.condition()?})
    }
}
pub(super) fn load(bytes:&[u8])->Result<Sequence> {
    let mut r=Reader {bytes,at:0};ensure!(r.word()?==44,"Expected Apprentice version44");
    let sequence=r.sequence()?;ensure!(r.at==bytes.len(),"Unparsed Apprentice44 bytes at {:#x}/{}",r.at,bytes.len());Ok(sequence)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pc_padded_boolean_ignores_three_padding_bytes()->Result<()> {
        for (bytes,expected) in [([0,0,0,0],false),([1,0,0,0],true),([255,255,255,255],true),([0,255,205,205],false)] {
            let mut reader=Reader {bytes:&bytes,at:0};assert_eq!(reader.boolean()?,expected);assert_eq!(reader.at,4);
        }Ok(())
    }
    #[test]
    fn jps_string_and_padded_short_validate_their_own_spans()->Result<()> {
        let bytes=[4,0,205,205,1,0,255,255];let mut reader=Reader {bytes:&bytes,at:0};
        assert_eq!(reader.string()?,None);assert_eq!(reader.short()?,1);assert_eq!(reader.at,8);
        for bytes in [&[3,1,0,0][..],&[8,7,65,0][..],&[4,2,65,66][..]] {assert!(Reader {bytes,at:0}.string().is_err());}
        Ok(())
    }
}
