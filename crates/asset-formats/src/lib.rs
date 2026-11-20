//! # epic-mickey-lib-rs
//!
//! A Rust library for reading and writing the file formats used by the
//! *Epic Mickey* series of games (built on the Gamebryo engine).
//!
//! ## Supported games
//!
//! - Epic Mickey \[Demo\] (Wii)
//! - Epic Mickey (Wii)
//! - Epic Mickey \[JP\] (Wii)
//! - Epic Mickey 2: The Power of Two \[Prototypes\] (Wii)
//! - Epic Mickey 2: The Power of Two (Wii, PS3, Xbox 360, PC, Wii U, PS Vita)
//!
//! ## Supported formats
//!
//! | Module | Format | Description |
//! |---|---|---|
//! | [`collectible_database`] | `.cdb` | Collectibles & extras database |
//! | [`dct`] | `.dct` | Localised dialog dictionary |
//! | [`packfile`] | `.pak` | Asset pack container |
//! | [`scene_file`] | `.scn` | Scene / level data |
//! | [`sequence_file`] | `.seq` | Scripted event sequences |
//!
//! ## Quick start
//!
//! ```no_run
//! use epic_mickey_lib_rs::dct::DCT;
//!
//! let dct = DCT::from_binary_path("EN_US.dct".to_string());
//! if let Some(text) = dct.get_line("HUD_PAINT_AMOUNT") {
//!     println!("{}", text);
//! }
//! ```

pub mod collectible_database;
pub mod dct;
pub mod ffi_anim;
pub mod file_manipulator;
pub mod hkx_file;
pub mod kfm_file;
pub mod nif_file;
pub mod packfile;
pub mod scene_file;
pub mod sequence_file;
