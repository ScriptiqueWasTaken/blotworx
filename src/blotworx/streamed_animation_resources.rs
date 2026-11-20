//! Lazy source archive identities for PC streamed cutscene animation.
use anyhow::{ensure,Context,Result};
use std::{collections::HashMap,path::PathBuf};

/// Header-only Wii clip lookup for a placed actor whose authored graph
/// refers to a clip omitted from the currently admitted scene archives.
pub(super) struct WiiClips {
    root: PathBuf,
    index: std::sync::OnceLock<Result<Catalog, String>>,
}

impl WiiClips {
    pub fn new(root: &std::path::Path) -> Self {
        Self { root: root.to_owned(), index: std::sync::OnceLock::new() }
    }

    pub fn read(&self, path: &str) -> Result<Option<Vec<u8>>> {
        let logical = super::resource_path::canonical(path.replace('\\', "/").trim_start_matches('/'))?;
        let catalog = self.index.get_or_init(|| self.build_index().map_err(|error| format!("{error:#}")))
            .as_ref().map_err(|error| anyhow::anyhow!("{error}"))?;
        catalog.read(&format!("{logical}_wii"))
    }

    fn build_index(&self) -> Result<Catalog> {
        let mut paths = std::fs::read_dir(self.root.join("packfiles"))?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        paths.retain(|path| path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("pak")));
        paths.sort_unstable();
        let mut catalog = Catalog::default();
        for path in paths {
            for member in crate::pack_io::index(&path)? {
                let key = member.path.replace('\\', "/").trim_start_matches('/').to_ascii_lowercase();
                if key.ends_with(".hkx_wii") {
                    catalog.entries.entry(key).or_insert_with(|| (path.clone(), member));
                }
            }
        }
        Ok(catalog)
    }
}

#[derive(Default)]
pub(super) struct Catalog {entries:HashMap<String,(PathBuf,crate::pack_io::Member)>,chunks:Option<super::animation_chunks::Archive>}
impl Catalog {
    pub fn mount(&mut self,path:&std::path::Path)->Result<()> {
        for member in crate::pack_io::index(path)? {
            let key=member.path.replace('\\',"/").to_ascii_lowercase();
            if key.ends_with(".chk")||key.ends_with(".hkx") {self.entries.entry(key).or_insert_with(||(path.to_owned(),member));}
        }Ok(())
    }
    pub fn mount_chunks(&mut self,path:&std::path::Path)->Result<()> {
        ensure!(self.chunks.is_none(),"Animation chunk archive already mounted");
        self.chunks=Some(super::animation_chunks::Archive::open(path)?);Ok(())
    }
    pub fn read(&self,key:&str)->Result<Option<Vec<u8>>> {
        if let Some((path,member))=self.entries.get(&key.to_ascii_lowercase()){return Ok(Some(member.read(path)?));}
        if key.to_ascii_lowercase().ends_with(".chk") {if let Some(chunks)=&self.chunks{return chunks.read(key);}}
        Ok(None)
    }
}
/// Original PC b6d7d0: normalize the directory, retain /characters/ or
/// /lipsync/, seed CRC32 with its byte length, then build the root chunk name.
pub(super) fn cache_key(logical:&str)->Result<String> {
    ensure!(logical.is_ascii(),"Non-ASCII streamed resource paths need native locale support");
    let normalized=format!("/{}",logical.replace('\\',"/").trim_start_matches('/')).to_ascii_lowercase();
    ensure!(!normalized.split('/').any(|s|s=="..")&&!normalized.contains(':'),"Unsupported streamed resource path");
    let (directory,name)=normalized.rsplit_once('/').context("Streamed resource has no directory")?;
    let stem=name.strip_suffix(".hkx").context("Streamed resource is not HKX")?;
    let directory=format!("{directory}/");
    let begin=directory.find("/characters/").or_else(||directory.find("/lipsync/")).context("Streamed resource has no original character/lipsync root")?;
    let directory=&directory[begin..];
    let stem=if directory.contains("/lipsync/"){stem.to_owned()}else{
        let mut crc=!(directory.len() as u32);
        for byte in directory.bytes(){crc^=byte as u32;for _ in 0..8{crc=(crc>>1)^(0xedb88320u32.wrapping_mul(crc&1));}}
        format!("{stem}_{:08x}",!crc)
    };
    Ok(format!("animations/{stem}.000.chk"))
}
