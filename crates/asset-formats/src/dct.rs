// dct.rs
// © 2024 Epic Mickey Library

use crate::file_manipulator;
use serde_json;
use std::{
    fs::File,
    io::{Read, Write},
};

#[cfg(test)]
mod hash_tests {
    #[test]
    fn short_and_partial_dialogue_keys_hash_without_panicking() {
        // Exercise every partial-word boundary, including exact 12-byte blocks.
        for length in 0..=48 {
            let lower=vec![b'a';length];let upper=vec![b'A';length];
            assert_eq!(super::epic_mickey_hash(&lower,0x12345678),super::epic_mickey_hash(&upper,0x12345678));
        }
        assert_eq!(super::epic_mickey_hash(b"",0),0xbd49d10d);
        assert_ne!(super::epic_mickey_hash(b"ab",0),super::epic_mickey_hash(b"abc",0));
    }
}

/// Hashes a dialog key string using the Epic Mickey hash algorithm.
///
/// This is a direct port of the hash function found at address `80A1D070` in the
/// US release's secondary relocatable executable. It uses a Jenkins-style
/// three-variable mix applied over 12-byte chunks of the input.
///
/// # Arguments
///
/// * `string` - The raw bytes of the string to hash. Lowercasing is performed
///   internally on a byte-by-byte basis (ASCII lowercase only), matching the
///   behaviour of Python's `bytes.lower()`.
/// * `seed` - The unique hash seed for the dictionary file. This is the value
///   stored at offset `0x8` in a `.DCT` file (the `hash_seed` field).
///
/// # Examples
///
/// ```
/// use epic_mickey_lib_rs::dct::epic_mickey_hash;
/// let hash = epic_mickey_hash(b"MY_DIALOG_KEY", 0x0);
/// ```
pub fn epic_mickey_hash(string: &[u8], seed: u32) -> u32 {
    /// Wrapping helper that mirrors Python's `uint32 = lambda num: num & 0xFFFF_FFFF`.
    #[inline(always)]
    fn u32w(n: u64) -> u32 {
        n as u32
    }

    // ASCII lowercase, byte-by-byte (mirrors Python bytes.lower()).
    let string: Vec<u8> = string.iter().map(|b| b.to_ascii_lowercase()).collect();
    let len = string.len();
    let chunks = len / 12;

    let mut hash0 = seed as u64;
    let mut hash1: u64 = 0x9E37_79B9;
    let mut hash2: u64 = 0x9E37_79B9;

    let read_le32 = |bytes: &[u8], start: usize| -> u64 {
        let s = &bytes[start..];
        u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as u64
    };

    let mut idx = 0usize;
    for _ in 0..chunks {
        let hash4 = u32w(hash0 + read_le32(&string, idx + 8));
        let hash3 = u32w(hash1 + read_le32(&string, idx + 4));
        hash1 = u32w(hash2 + read_le32(&string, idx)) as u64;

        hash1 = u32w(
            (hash1.wrapping_sub(hash3 as u64).wrapping_sub(hash4 as u64)) ^ ((hash4 as u64) >> 13),
        ) as u64;
        let hash3 = u32w(
            (hash3 as u64)
                .wrapping_sub(hash4 as u64)
                .wrapping_sub(hash1)
                ^ (hash1 << 8),
        );
        hash2 = u32w(
            (hash4 as u64)
                .wrapping_sub(hash1)
                .wrapping_sub(hash3 as u64)
                ^ ((hash3 as u64) >> 13),
        ) as u64;
        hash1 = u32w(hash1.wrapping_sub(hash3 as u64).wrapping_sub(hash2) ^ (hash2 >> 12)) as u64;
        let hash3 = u32w((hash3 as u64).wrapping_sub(hash2).wrapping_sub(hash1) ^ (hash1 << 16));
        let hash4 =
            u32w(hash2.wrapping_sub(hash1).wrapping_sub(hash3 as u64) ^ ((hash3 as u64) >> 5));
        hash2 = u32w(
            hash1.wrapping_sub(hash3 as u64).wrapping_sub(hash4 as u64) ^ ((hash4 as u64) >> 3),
        ) as u64;
        hash1 = u32w(
            (hash3 as u64)
                .wrapping_sub(hash4 as u64)
                .wrapping_sub(hash2)
                ^ (hash2 << 10),
        ) as u64;
        hash0 = u32w((hash4 as u64).wrapping_sub(hash2).wrapping_sub(hash1) ^ (hash1 >> 15)) as u64;

        idx += 12;
    }

    // Read up to 3 bytes of the 3rd partial word (bytes [idx+8 .. idx+11]),
    // shifted left by 8 bits, matching the Python slice `string[idx+8:idx+11]`.
    let tail3 = {
        let mut buf = [0u8; 4];
        let avail = len.saturating_sub(idx + 8).min(3);
        if avail != 0 {
            buf[..avail].copy_from_slice(&string[idx + 8..idx + 8 + avail]);
        }
        (u32::from_le_bytes(buf) as u64) << 8
    };
    // Full 4-byte reads for the 1st and 2nd partial words, zero-padded at end.
    let tail1 = {
        let mut buf = [0u8; 4];
        let avail = len.saturating_sub(idx + 4).min(4);
        if avail != 0 {
            buf[..avail].copy_from_slice(&string[idx + 4..idx + 4 + avail]);
        }
        u32::from_le_bytes(buf) as u64
    };
    let tail0 = {
        let mut buf = [0u8; 4];
        let avail = len.saturating_sub(idx).min(4);
        buf[..avail].copy_from_slice(&string[idx..idx + avail]);
        u32::from_le_bytes(buf) as u64
    };

    hash0 = u32w(hash0 + len as u64 + tail3) as u64;
    hash1 = u32w(hash1 + tail1) as u64;
    hash2 = u32w(hash2 + tail0) as u64;

    hash2 = u32w(hash2.wrapping_sub(hash1).wrapping_sub(hash0) ^ (hash0 >> 13)) as u64;
    let hash4 = u32w(hash1.wrapping_sub(hash0).wrapping_sub(hash2) ^ (hash2 << 8));
    let hash3 = u32w(hash0.wrapping_sub(hash2).wrapping_sub(hash4 as u64) ^ ((hash4 as u64) >> 13));
    hash1 =
        u32w(hash2.wrapping_sub(hash4 as u64).wrapping_sub(hash3 as u64) ^ ((hash3 as u64) >> 12))
            as u64;
    let hash4 = u32w(
        (hash4 as u64)
            .wrapping_sub(hash3 as u64)
            .wrapping_sub(hash1)
            ^ (hash1 << 16),
    );
    hash2 = u32w(
        (hash3 as u64)
            .wrapping_sub(hash1)
            .wrapping_sub(hash4 as u64)
            ^ ((hash4 as u64) >> 5),
    ) as u64;
    hash1 = u32w(hash1.wrapping_sub(hash4 as u64).wrapping_sub(hash2) ^ (hash2 >> 3)) as u64;
    let hash3 = u32w((hash4 as u64).wrapping_sub(hash2).wrapping_sub(hash1) ^ (hash1 << 10));
    hash0 =
        u32w(hash2.wrapping_sub(hash1).wrapping_sub(hash3 as u64) ^ ((hash3 as u64) >> 15)) as u64;

    hash0 as u32
}

/// Represents a single entry in a DCT file.
///
/// # Fields
///
/// * `hashed_key` - The hashed key of the entry.
/// * `text` - The text of the entry.
pub struct DialogEntry {
    pub hashed_key: u32,
    pub text: String,
}

impl DialogEntry {
    pub fn new(hashed_key: u32, text: String) -> DialogEntry {
        DialogEntry { hashed_key, text }
    }

    pub fn to_dict(&self) -> serde_json::Value {
        let mut dict = serde_json::Map::new();
        dict.insert(
            "hashed_key".to_owned(),
            serde_json::Value::Number(serde_json::Number::from(self.hashed_key)),
        );
        dict.insert(
            "text".to_owned(),
            serde_json::Value::String(self.text.clone()),
        );
        return serde_json::Value::Object(dict);
    }

    pub fn from_dict(dict: serde_json::Value) -> DialogEntry {
        let hashed_key = dict["hashed_key"].as_u64().unwrap() as u32;
        let text = dict["text"].as_str().unwrap().to_owned();
        return DialogEntry::new(hashed_key, text);
    }
}

/// Represents a footer entry in a DCT file.
///
/// # Fields
///
/// * `number` - The number of the entry.
/// * `text` - The text of the entry.
pub struct FooterEntry {
    pub number: u32,
    pub text: String,
}

impl FooterEntry {
    pub fn new(number: u32, text: String) -> FooterEntry {
        FooterEntry { number, text }
    }

    pub fn to_dict(&self) -> serde_json::Value {
        let mut dict = serde_json::Map::new();
        dict.insert(
            "number".to_owned(),
            serde_json::Value::Number(serde_json::Number::from(self.number)),
        );
        dict.insert(
            "text".to_owned(),
            serde_json::Value::String(self.text.clone()),
        );
        return serde_json::Value::Object(dict);
    }

    pub fn from_dict(dict: serde_json::Value) -> FooterEntry {
        let number = dict["number"].as_u64().unwrap() as u32;
        let text = dict["text"].as_str().unwrap().to_owned();
        return FooterEntry::new(number, text);
    }
}

/// Represents a DCT file.
///
/// # Fields
///
/// * `magic` - The magic of the DCT file.
/// * `version1` - The first version of the DCT file.
/// * `hash_seed` - The hash seed of the DCT file.
/// * `version2` - The second version of the DCT file.
/// * `dialog_entries` - The dialog entries of the DCT file.
/// * `footer_entries` - The footer entries of the DCT file.
pub struct DCT {
    pub magic: String,
    pub version1: u32,
    pub hash_seed: u32,
    pub version2: u32,
    pub dialog_entries: Vec<DialogEntry>,
    pub footer_entries: Vec<FooterEntry>,
}

impl DCT {
    pub fn new(
        magic: String,
        version1: u32,
        hash_seed: u32,
        version2: u32,
        dialog_entries: Vec<DialogEntry>,
        footer_entries: Vec<FooterEntry>,
    ) -> DCT {
        DCT {
            magic,
            version1,
            hash_seed,
            version2,
            dialog_entries,
            footer_entries,
        }
    }

    pub fn unpack(&mut self, fm: &mut file_manipulator::FileManipulator) {
        self.magic = fm.r_str(4);
        self.version1 = fm.r_u32();
        self.hash_seed = fm.r_u32();
        self.version2 = fm.r_u32();

        let num_dialog_entries = fm.r_u32();

        fm.move_pos(4);

        let footer_offset = fm.tell() as u32 + fm.r_u32() + 9;

        let mut has_footer = false;
        let footer_switch = fm.r_u32();
        if footer_switch == 1 {
            has_footer = true;
        }

        let mut current_data_offset;

        self.dialog_entries = Vec::new();
        for _ in 0..num_dialog_entries {
            let hashed_key = fm.r_u32();
            if hashed_key == 0 {
                fm.move_pos(8);
                // add empty dialog entry
                self.dialog_entries.push(DialogEntry::new(0, "".to_owned()));
                continue;
            }
            let line_offset = fm.tell() as u32 + fm.r_u32() + 1;
            let _line_zero = fm.r_u32();
            let current_data_offset = fm.tell();
            fm.seek(line_offset as usize);
            let line_text = fm.r_str_null();
            self.dialog_entries
                .push(DialogEntry::new(hashed_key, line_text));
            fm.seek(current_data_offset);
        }
        self.footer_entries = Vec::new();
        if has_footer {
            while fm.tell() < footer_offset as usize {
                let footer_line_offset = fm.tell() as u32 + fm.r_u32() + 1;
                let footer_line_id = fm.r_u32();
                current_data_offset = fm.tell();
                fm.seek(footer_line_offset as usize);
                let footer_line_text = fm.r_str_null();
                self.footer_entries
                    .push(FooterEntry::new(footer_line_id, footer_line_text));
                fm.seek(current_data_offset);
            }
        }
    }

    pub fn pack(&self) -> Vec<u8> {
        let mut fm = file_manipulator::FileManipulator::new(
            Vec::new(),
            file_manipulator::EndianType::LITTLE,
            file_manipulator::WriteMode::OVERWRITE,
        );

        let num_dialog_entries = self.dialog_entries.len() as u32;
        let num_footer_entries = self.footer_entries.len() as u32;
        let end_offset = (num_dialog_entries * 12) + (num_footer_entries * 8) - 1;

        fm.w_str(&self.magic);
        fm.w_u32(self.version1);
        fm.w_u32(self.hash_seed);
        fm.w_u32(self.version2);

        fm.w_u32(num_dialog_entries);

        fm.w_u32(1);
        fm.w_u32(end_offset);

        if num_footer_entries > 0 {
            fm.w_u32(1);
        } else {
            fm.w_u32(0);
        }

        let mut current_data_offset = fm.tell();
        let mut current_line_offset = end_offset + 50;

        for dialog_entry in &self.dialog_entries {
            fm.seek(current_data_offset);
            if dialog_entry.hashed_key == 0 {
                fm.w_u32(0);
                fm.w_u32(0);
                fm.w_u32(0);
                current_data_offset = fm.tell();
                continue;
            }
            fm.w_u32(dialog_entry.hashed_key);
            fm.w_u32(current_line_offset - fm.tell() as u32 - 1);
            fm.w_u32(0);
            current_data_offset = fm.tell();
            fm.seek(current_line_offset as usize);

            fm.w_str_null(&dialog_entry.text);
            current_line_offset = fm.tell() as u32;
            fm.seek(current_data_offset);
        }

        if num_footer_entries > 0 {
            for footer_entry in &self.footer_entries {
                fm.w_u32(current_line_offset - fm.tell() as u32 - 1);
                fm.w_u32(footer_entry.number);
                current_data_offset = fm.tell();
                fm.seek(current_line_offset as usize);
                fm.w_str_null(&footer_entry.text);
                current_line_offset = fm.tell() as u32;
                fm.seek(current_data_offset);
            }
            fm.write(&[0xDF, 0xFF, 0xFF, 0xFF]);
            fm.w_u32(11);
            fm.w_u32(12);
            fm.w_u32(0);
        }

        // return data
        return fm.get_data().to_vec();
    }

    pub fn to_binary(&self) -> Vec<u8> {
        return self.pack();
    }

    pub fn to_binary_path(&self, path: String) {
        let mut file = File::create(path).unwrap();
        file.write_all(&self.pack()).unwrap();
    }

    pub fn to_dict(&self) -> serde_json::Value {
        let mut dict = serde_json::Map::new();
        dict.insert(
            "magic".to_owned(),
            serde_json::Value::String(self.magic.clone()),
        );
        dict.insert(
            "version1".to_owned(),
            serde_json::Value::Number(serde_json::Number::from(self.version1)),
        );
        dict.insert(
            "hash_seed".to_owned(),
            serde_json::Value::Number(serde_json::Number::from(self.hash_seed)),
        );
        dict.insert(
            "version2".to_owned(),
            serde_json::Value::Number(serde_json::Number::from(self.version2)),
        );
        let mut dialog_entries = Vec::new();
        for dialog_entry in &self.dialog_entries {
            dialog_entries.push(dialog_entry.to_dict());
        }
        dict.insert(
            "dialog_entries".to_owned(),
            serde_json::Value::Array(dialog_entries),
        );
        let mut footer_entries = Vec::new();
        for footer_entry in &self.footer_entries {
            footer_entries.push(footer_entry.to_dict());
        }
        dict.insert(
            "footer_entries".to_owned(),
            serde_json::Value::Array(footer_entries),
        );
        return serde_json::Value::Object(dict);
    }

    pub fn to_json(&self) -> String {
        return serde_json::to_string_pretty(&self.to_dict()).unwrap();
    }

    pub fn to_json_path(&self, path: String) {
        let mut file = File::create(path).unwrap();
        file.write_all(self.to_json().as_bytes()).unwrap();
    }

    pub fn from_binary(data: Vec<u8>) -> DCT {
        let mut fm = file_manipulator::FileManipulator::new(
            data,
            file_manipulator::EndianType::LITTLE,
            file_manipulator::WriteMode::OVERWRITE,
        );
        let mut dct = DCT::new("".to_owned(), 0, 0, 0, Vec::new(), Vec::new());
        dct.unpack(&mut fm);
        return dct;
    }

    pub fn from_binary_path(path: String) -> DCT {
        // check if file exists
        if !std::path::Path::new(&path).exists() {
            panic!("File does not exist!");
        }
        let mut file = File::open(path).unwrap();
        let mut data = Vec::new();
        file.read_to_end(&mut data).unwrap();
        return DCT::from_binary(data);
    }

    pub fn from_dict(dict: serde_json::Value) -> DCT {
        let magic = dict["magic"].as_str().unwrap().to_owned();
        let version1 = dict["version1"].as_u64().unwrap() as u32;
        let hash_seed = dict["hash_seed"].as_u64().unwrap() as u32;
        let version2 = dict["version2"].as_u64().unwrap() as u32;
        let mut dialog_entries = Vec::new();
        for dialog_entry in dict["dialog_entries"].as_array().unwrap() {
            dialog_entries.push(DialogEntry::from_dict(dialog_entry.clone()));
        }
        let mut footer_entries = Vec::new();
        for footer_entry in dict["footer_entries"].as_array().unwrap() {
            footer_entries.push(FooterEntry::from_dict(footer_entry.clone()));
        }
        return DCT::new(
            magic,
            version1,
            hash_seed,
            version2,
            dialog_entries,
            footer_entries,
        );
    }

    pub fn set_line_from_hash(&mut self, hashed_key: u32, text: String) {
        // if key is 0, ignore
        if hashed_key == 0 {
            return;
        }
        for dialog_entry in &mut self.dialog_entries {
            if dialog_entry.hashed_key == hashed_key {
                dialog_entry.text = text;
                return;
            }
        }
        self.dialog_entries.push(DialogEntry::new(hashed_key, text));
    }

    /// Hashes `key` using this file's own [`hash_seed`](DCT::hash_seed).
    ///
    /// This is a shorthand for calling [`epic_mickey_hash`] with `self.hash_seed`
    /// as the seed. The returned value is the same 32-bit hash that is stored in
    /// [`DialogEntry::hashed_key`].
    pub fn hash_key(&self, key: &str) -> u32 {
        epic_mickey_hash(key.as_bytes(), self.hash_seed)
    }

    /// Returns the dialog text for the given string key, or `None` if not found.
    ///
    /// The key is hashed with [`hash_key`](DCT::hash_key) before searching
    /// `dialog_entries`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use epic_mickey_lib_rs::dct::DCT;
    /// let dct = DCT::from_binary_path("EN_US.dct".to_string());
    /// if let Some(text) = dct.get_line("HUD_PAINT_AMOUNT") {
    ///     println!("{}", text);
    /// }
    /// ```
    pub fn get_line(&self, key: &str) -> Option<&str> {
        let hash = self.hash_key(key);
        self.dialog_entries
            .iter()
            .find(|e| e.hashed_key == hash)
            .map(|e| e.text.as_str())
    }

    /// Inserts or replaces the dialog entry for the given string key.
    ///
    /// The key is hashed with [`hash_key`](DCT::hash_key). If an entry with the
    /// resulting hash already exists, its text is updated; otherwise a new entry
    /// is appended.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use epic_mickey_lib_rs::dct::DCT;
    /// let mut dct = DCT::from_binary_path("EN_US.dct".to_string());
    /// dct.set_line("HUD_PAINT_AMOUNT", "Paint: {0}".to_string());
    /// ```
    pub fn set_line(&mut self, key: &str, text: String) {
        let hash = self.hash_key(key);
        self.set_line_from_hash(hash, text);
    }

    pub fn merge_in_dict(&mut self, dict: serde_json::Value) {
        // if there is a magic, overwrite it
        if dict["magic"].is_string() {
            self.magic = dict["magic"].as_str().unwrap().to_owned();
        }
        // if there is a version1, overwrite it
        if dict["version1"].is_u64() {
            self.version1 = dict["version1"].as_u64().unwrap() as u32;
        }
        // if there is a hash_seed, overwrite it
        if dict["hash_seed"].is_u64() {
            self.hash_seed = dict["hash_seed"].as_u64().unwrap() as u32;
        }
        // if there is a version2, overwrite it
        if dict["version2"].is_u64() {
            self.version2 = dict["version2"].as_u64().unwrap() as u32;
        }
        // if there are dialog_entries, overwrite any existing ones, or add new ones
        if dict["dialog_entries"].is_array() {
            for dialog_entry in dict["dialog_entries"].as_array().unwrap() {
                let hashed_key = dialog_entry["hashed_key"].as_u64().unwrap() as u32;
                let text = dialog_entry["text"].as_str().unwrap().to_owned();
                self.set_line_from_hash(hashed_key, text);
            }
        }
        // if there are footer_entries, append them
        if dict["footer_entries"].is_array() {
            for footer_entry in dict["footer_entries"].as_array().unwrap() {
                let number = footer_entry["number"].as_u64().unwrap() as u32;
                let text = footer_entry["text"].as_str().unwrap().to_owned();
                self.footer_entries.push(FooterEntry::new(number, text));
            }
        }
    }

    pub fn merge_in_json(&mut self, json: String) {
        let dict = serde_json::from_str(&json).unwrap();
        self.merge_in_dict(dict);
    }

    pub fn merge_in_json_path(&mut self, path: String) {
        // check if file exists
        if !std::path::Path::new(&path).exists() {
            panic!("File does not exist!");
        }
        let mut file = File::open(path).unwrap();
        let mut json = String::new();
        file.read_to_string(&mut json).unwrap();
        self.merge_in_json(json);
    }

    pub fn from_json(json: String) -> DCT {
        let dict = serde_json::from_str(&json).unwrap();
        return DCT::from_dict(dict);
    }

    pub fn from_json_path(path: String) -> DCT {
        // check if file exists
        if !std::path::Path::new(&path).exists() {
            panic!("File does not exist!");
        }
        let mut file = File::open(path).unwrap();
        let mut json = String::new();
        file.read_to_string(&mut json).unwrap();
        return DCT::from_json(json);
    }
}
