//! PC LVLC v9 graph. Records and strings retain their authored order.
use anyhow::{ensure, Context, Result};
use std::collections::HashSet;

struct Reader<'a> { bytes:&'a [u8], at:usize, pool:usize }
impl Reader<'_> {
    fn word(&mut self)->Result<u32> {
        let end=self.at.checked_add(4).context("LVLC record overflow")?;
        ensure!(end<=self.pool,"Compiled level record crosses string pool");
        let value=u32::from_le_bytes(self.bytes[self.at..end].try_into()?);
        self.at=end;Ok(value)
    }
    fn count(&mut self,minimum:usize)->Result<usize> {
        let count=self.word()? as usize;
        ensure!(count<=(self.pool-self.at)/minimum,"Compiled level count {count} at {} exceeds remaining {} bytes (minimum {minimum})",self.at-4,self.pool-self.at);
        Ok(count)
    }
    fn string(&mut self)->Result<String> {
        let offset=self.word()? as usize;
        let start=self.pool.checked_add(offset).context("Compiled level string overflow")?;
        let tail=self.bytes.get(start..).context("Invalid compiled level string")?;
        let end=tail.iter().position(|&b|b==0).context("Unterminated compiled level string")?;
        Ok(std::str::from_utf8(&tail[..end])?.to_owned())
    }
}
#[derive(Clone,serde::Serialize)]
pub(super) struct Resource {pub path:String,pub alias:String,pub reload:u32}
#[derive(Clone,serde::Serialize)]
pub(super) struct Zone {pub name:String,pub folder:String,pub metadata:[u32;5],pub files:Vec<Resource>,pub dependencies:Vec<String>}
#[derive(Clone,serde::Serialize)]
pub(super) struct Link {pub kind:u32,pub zone:String,pub flag:u32}
#[derive(Clone,serde::Serialize)]
pub(super) struct Destination {pub state:String,pub flag:u32}
#[derive(Clone,serde::Serialize)]
pub(super) struct State {pub name:String,pub level:String,pub links:Vec<Link>,pub destinations:Vec<Destination>}
#[derive(serde::Serialize)]
pub(super) struct Level {pub version:u32,pub trusted_measurements:u32,pub zones:Vec<Zone>,pub states:Vec<State>}
impl Level {
    pub fn parse(bytes:&[u8])->Result<Self> {
        ensure!(bytes.len()>=16,"Truncated compiled level header");
        let pool=u32::from_le_bytes(bytes[8..12].try_into()?) as usize;
        ensure!(pool>=16 && pool<=bytes.len(),"Invalid compiled level string pool");
        let mut r=Reader {bytes,at:0,pool};let version=r.word()?;
        ensure!(version==9,"Unsupported EM2 compiled level version {version}");
        let trusted_measurements=r.word()?;let _pool=r.word()?;
        let mut zones=Vec::new();
        for _ in 0..r.count(36)? {
            let name=r.string()?;let folder=r.string()?;
            let mut metadata=[0;5];for value in &mut metadata {*value=r.word()?;}
            let mut files=Vec::new();for _ in 0..r.count(12)? {files.push(Resource {path:r.string()?,alias:r.string()?,reload:r.word()?});}
            let mut dependencies=Vec::new();for _ in 0..r.count(4)? {dependencies.push(r.string()?);}
            zones.push(Zone {name,folder,metadata,files,dependencies});
        }
        let mut states=Vec::new();
        for _ in 0..r.count(12)? {
            let name=r.string()?;let level=r.string()?;let mut links=Vec::new();
            for _ in 0..r.count(12)? {links.push(Link {kind:r.word()?,zone:r.string()?,flag:r.word()?});}
            states.push(State {name,level,links,destinations:Vec::new()});
        }
        let mut destination_owners=HashSet::new();
        for _ in 0..states.len() {
            let name=r.string()?;
            ensure!(destination_owners.insert(name.to_ascii_lowercase()),"Repeated LVLC destination owner {name}");
            let state=states.iter_mut().find(|s|s.name.eq_ignore_ascii_case(&name)).with_context(||format!("Unknown LVLC destination owner {name}"))?;
            for _ in 0..r.count(8)? {state.destinations.push(Destination {state:r.string()?,flag:r.word()?});}
        }
        ensure!(r.at==pool,"Unparsed compiled level records at {}..{pool}",r.at);
        let mut names=HashSet::new();for zone in &zones {ensure!(names.insert(zone.name.to_ascii_lowercase()),"Duplicate LVLC zone {}",zone.name);}
        let mut state_names=HashSet::new();for state in &states {ensure!(state_names.insert(state.name.to_ascii_lowercase()),"Duplicate LVLC state {}",state.name);}
        // Production graphs reference zones/states owned by other LVLCs.
        // Resolve identities against the mounted catalog, not this one file.
        Ok(Self {version,trusted_measurements,zones,states})
    }
}
