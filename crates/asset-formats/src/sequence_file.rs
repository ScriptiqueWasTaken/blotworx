// sequence_file.rs
// © 2024 Epic Mickey Library

//! Parsing and serialisation of Epic Mickey `.bsq` sequence files.
//!
//! Sequence files drive in-game scripted events: dialog conversations, quest
//! updates, inventory changes, camera cuts, and more. Each file contains a
//! single [`Sequence`] (a named list of [`SequenceEvent`]s) prefixed by a
//! 4-byte version number.
//!
//! # Quick start
//!
//! ```no_run
//! use epic_mickey_lib_rs::file_manipulator::EndianType;
//! use epic_mickey_lib_rs::sequence_file::SequenceFile;
//! let sf = SequenceFile::from_path("my_sequence.bsq", EndianType::BIG).unwrap();
//! println!("Sequence name: {}", sf.sequence.name);
//! for event in &sf.sequence.events {
//!     println!("  event: {:?}", event);
//! }
//! ```

use crate::file_manipulator::{EndianType, FileManipulator, WriteMode};
use std::{fmt, fs, io};

// ─────────────────────────────────────────────────────────────────────────────
// Error type
// ─────────────────────────────────────────────────────────────────────────────

/// Errors that can arise while parsing or serialising a sequence file.
#[derive(Debug)]
pub enum SequenceFileError {
    /// The event type tag found in the file is not recognised.
    UnknownEventType(u16),
    /// The expression type tag found in the file is not recognised.
    UnknownExpressionType(u16),
    /// Event type 13 or 14, which are known to be deprecated in the game code.
    DeprecatedEventType(u16),
    /// A validated enum field contained an unrecognised string value.
    InvalidEnumValue { field: &'static str, value: String },
    /// An I/O error occurred while reading from or writing to a path.
    Io(io::Error),
}

impl fmt::Display for SequenceFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownEventType(t) => write!(f, "Unknown sequence event type: {t}"),
            Self::UnknownExpressionType(t) => write!(f, "Unknown sequence expression type: {t}"),
            Self::DeprecatedEventType(t) => write!(f, "Deprecated sequence event type: {t}"),
            Self::InvalidEnumValue { field, value } => {
                write!(f, "Invalid value {value:?} for field '{field}'")
            }
            Self::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

impl std::error::Error for SequenceFileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for SequenceFileError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helper – write a byte buffer to a fresh FileManipulator and return the data
// ─────────────────────────────────────────────────────────────────────────────

fn new_fm(endian: EndianType) -> FileManipulator {
    FileManipulator::new(Vec::new(), endian, WriteMode::OVERWRITE)
}

// ─────────────────────────────────────────────────────────────────────────────
// Expressions (condition tree nodes)
// ─────────────────────────────────────────────────────────────────────────────

/// A node in the sequence condition expression tree.
///
/// Expressions are stored with a 16-bit type tag (JPS u16) followed by
/// type-specific payload. `And` and `Or` contain nested child expressions,
/// making this structure recursive.
///
/// Type tag mapping (matches the C# `GetExpressionType` switch):
///
/// | Tag | Variant |
/// |-----|---------|
/// | 0   | `And`      |
/// | 1   | `Or`       |
/// | 2   | `State`    |
/// | 3   | `Mood`     |
/// | 4   | `Currency` |
/// | 5   | `Item`     |
/// | 6   | `Quest`    |
#[derive(Debug, Clone)]
pub enum SequenceExpression {
    /// Logical AND of child expressions.
    And {
        expressions: Vec<SequenceExpression>,
    },
    /// Logical OR of child expressions.
    Or {
        expressions: Vec<SequenceExpression>,
    },
    /// Tests a named game state variable.
    State {
        /// The state variable name.
        state_name: String,
        /// Unknown u32 field 1.
        unknown_u32_1: u32,
        /// Unknown u32 field 2.
        unknown_u32_2: u32,
    },
    /// Tests the current character mood.
    Mood {
        /// The mood name string.
        mood_name: String,
    },
    /// Tests a currency amount condition.
    ///
    /// Valid currency values: `"ETicket"`, `"Pins"`, `"ScrapMetal"`, `"Cloth"`.
    Currency {
        /// The currency type identifier.
        currency: String,
        /// Unknown signed short field.
        unknown_i16: i16,
        /// Unknown unsigned short field.
        unknown_u16: u16,
    },
    /// Tests whether the player has a specific item.
    Item {
        /// The item identifier string.
        item: String,
        /// Unknown signed short field.
        unknown_i16: i16,
        /// Unknown unsigned short field.
        unknown_u16: u16,
    },
    /// Tests the state of a quest.
    ///
    /// Valid states: `"Undiscovered"`, `"Discovered"`, `"TurnIn"`,
    /// `"Complete"`, `"Incomplete"`, `"HaveAllItems"`, `"Unavailable"`.
    Quest {
        /// The quest name / category string.
        quest_name: String,
        /// A secondary quest identifier string.
        quest_name2: String,
        /// The quest state to test.
        state: String,
    },
}

/// Valid currency values accepted by [`SequenceExpression::Currency`].
pub const VALID_CURRENCIES: &[&str] = &["ETicket", "Pins", "ScrapMetal", "Cloth"];

/// Valid quest states accepted by [`SequenceExpression::Quest`].
pub const VALID_QUEST_STATES: &[&str] = &[
    "Undiscovered",
    "Discovered",
    "TurnIn",
    "Complete",
    "Incomplete",
    "HaveAllItems",
    "Unavailable",
];

impl SequenceExpression {
    /// Returns the binary type tag for this expression variant.
    pub fn type_tag(&self) -> u16 {
        match self {
            Self::And { .. } => 0,
            Self::Or { .. } => 1,
            Self::State { .. } => 2,
            Self::Mood { .. } => 3,
            Self::Currency { .. } => 4,
            Self::Item { .. } => 5,
            Self::Quest { .. } => 6,
        }
    }

    /// Deserialise an expression from the file stream (type tag already consumed by caller).
    fn unpack_body(tag: u16, fm: &mut FileManipulator) -> Result<Self, SequenceFileError> {
        match tag {
            0 | 1 => {
                let count = fm.r_u32();
                let mut exprs = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    exprs.push(Self::unpack(fm)?);
                }
                if tag == 0 {
                    Ok(Self::And { expressions: exprs })
                } else {
                    Ok(Self::Or { expressions: exprs })
                }
            }
            2 => Ok(Self::State {
                state_name: fm.r_str_jps(),
                unknown_u32_1: fm.r_u32(),
                unknown_u32_2: fm.r_u32(),
            }),
            3 => Ok(Self::Mood {
                mood_name: fm.r_str_jps(),
            }),
            4 => {
                let currency = fm.r_str_jps();
                if !VALID_CURRENCIES.contains(&currency.as_str()) {
                    return Err(SequenceFileError::InvalidEnumValue {
                        field: "currency",
                        value: currency,
                    });
                }
                Ok(Self::Currency {
                    currency,
                    unknown_i16: fm.r_s16_jps(),
                    unknown_u16: fm.r_u16_jps(),
                })
            }
            5 => Ok(Self::Item {
                item: fm.r_str_jps(),
                unknown_i16: fm.r_s16_jps(),
                unknown_u16: fm.r_u16_jps(),
            }),
            6 => {
                let quest_name = fm.r_str_jps();
                let quest_name2 = fm.r_str_jps();
                let state = fm.r_str_jps();
                if !VALID_QUEST_STATES.contains(&state.as_str()) {
                    return Err(SequenceFileError::InvalidEnumValue {
                        field: "quest_state",
                        value: state,
                    });
                }
                Ok(Self::Quest {
                    quest_name,
                    quest_name2,
                    state,
                })
            }
            other => Err(SequenceFileError::UnknownExpressionType(other)),
        }
    }

    /// Read a full expression (type-tag + body) from the file stream.
    pub fn unpack(fm: &mut FileManipulator) -> Result<Self, SequenceFileError> {
        let tag = fm.r_u16_jps();
        Self::unpack_body(tag, fm)
    }

    /// Serialise this expression to bytes (type-tag + body).
    pub fn pack(&self, endian: EndianType) -> Vec<u8> {
        let mut fm = new_fm(endian);
        fm.w_u16_jps(self.type_tag(), 1); // 0xFFFF padding
        fm.write(&self.pack_body(endian));
        fm.get_data().to_vec()
    }

    /// Serialise only the body (no type-tag).
    fn pack_body(&self, endian: EndianType) -> Vec<u8> {
        let mut fm = new_fm(endian);
        match self {
            Self::And { expressions } | Self::Or { expressions } => {
                fm.w_u32(expressions.len() as u32);
                for expr in expressions {
                    fm.write(&expr.pack(endian));
                }
            }
            Self::State {
                state_name,
                unknown_u32_1,
                unknown_u32_2,
            } => {
                fm.w_str_jps(state_name);
                fm.w_u32(*unknown_u32_1);
                fm.w_u32(*unknown_u32_2);
            }
            Self::Mood { mood_name } => {
                fm.w_str_jps(mood_name);
            }
            Self::Currency {
                currency,
                unknown_i16,
                unknown_u16,
            } => {
                fm.w_str_jps(currency);
                fm.w_s16_jps(*unknown_i16, 0);
                fm.w_u16_jps(*unknown_u16, 0);
            }
            Self::Item {
                item,
                unknown_i16,
                unknown_u16,
            } => {
                fm.w_str_jps(item);
                fm.w_s16_jps(*unknown_i16, 0);
                fm.w_u16_jps(*unknown_u16, 0);
            }
            Self::Quest {
                quest_name,
                quest_name2,
                state,
            } => {
                fm.w_str_jps(quest_name);
                fm.w_str_jps(quest_name2);
                fm.w_str_jps(state);
            }
        }
        fm.get_data().to_vec()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helper structs used by events
// ─────────────────────────────────────────────────────────────────────────────

/// Dialog line metadata read/written for dialog events.
#[derive(Debug, Clone, Default)]
pub struct DialogInfo {
    /// The speaker name localisation key.
    pub name_key: String,
    /// A secondary speaker name key.
    pub name_key2: String,
    /// The primary dialog text localisation key.
    pub text_key: String,
    /// Unknown string field.
    pub unknown_string: String,
    /// The speaker's emotional mood string.
    pub mood: String,
    /// Unknown boolean flag.
    pub unknown_bool: bool,
    /// A secondary text localisation key.
    pub text_key2: String,
    /// A tertiary text localisation key.
    pub text_key3: String,
}

impl DialogInfo {
    /// Deserialise from the file stream.
    pub fn unpack(fm: &mut FileManipulator) -> Self {
        Self {
            name_key: fm.r_str_jps(),
            name_key2: fm.r_str_jps(),
            text_key: fm.r_str_jps(),
            unknown_string: fm.r_str_jps(),
            mood: fm.r_str_jps(),
            unknown_bool: fm.r_bool(),
            text_key2: fm.r_str_jps(),
            text_key3: fm.r_str_jps(),
        }
    }

    /// Serialise to bytes.
    pub fn pack(&self, endian: EndianType) -> Vec<u8> {
        let mut fm = new_fm(endian);
        fm.w_str_jps(&self.name_key);
        fm.w_str_jps(&self.name_key2);
        fm.w_str_jps(&self.text_key);
        fm.w_str_jps(&self.unknown_string);
        fm.w_str_jps(&self.mood);
        fm.w_bool(self.unknown_bool);
        fm.w_str_jps(&self.text_key2);
        fm.w_str_jps(&self.text_key3);
        fm.get_data().to_vec()
    }
}

/// A single option in a choice event.
#[derive(Debug, Clone, Default)]
pub struct ChoiceEntry {
    /// The option's text key.
    pub text_key: String,
    /// A secondary key for the option.
    pub text_key2: String,
}

impl ChoiceEntry {
    /// Deserialise from the file stream.
    pub fn unpack(fm: &mut FileManipulator) -> Self {
        Self {
            text_key: fm.r_str_jps(),
            text_key2: fm.r_str_jps(),
        }
    }

    /// Serialise to bytes.
    pub fn pack(&self, endian: EndianType) -> Vec<u8> {
        let mut fm = new_fm(endian);
        fm.w_str_jps(&self.text_key);
        fm.w_str_jps(&self.text_key2);
        fm.get_data().to_vec()
    }
}

/// A named state variable paired with a short integer value.
#[derive(Debug, Clone, Default)]
pub struct StateValuePair {
    /// The global state variable name.
    pub state: String,
    /// The value to assign or compare against.
    pub value: i16,
}

impl StateValuePair {
    /// Deserialise from the file stream.
    pub fn unpack(fm: &mut FileManipulator) -> Self {
        Self {
            state: fm.r_str_jps(),
            value: fm.r_s16_jps(),
        }
    }

    /// Serialise to bytes.
    pub fn pack(&self, endian: EndianType) -> Vec<u8> {
        let mut fm = new_fm(endian);
        fm.w_str_jps(&self.state);
        fm.w_s16_jps(self.value, 1); // 0xFFFF padding
        fm.get_data().to_vec()
    }
}

/// One item offered in a sell event.
#[derive(Debug, Clone)]
pub struct SellItem {
    /// First item identifier string.
    pub item_name: String,
    /// Secondary item identifier string.
    pub item_name2: String,
    /// Unknown unsigned short field 1 (padded with `0xCDCD`).
    pub unknown_u16_1: u16,
    /// Unknown boolean flag 1.
    pub unknown_bool_1: bool,
    /// Unknown boolean flag 2.
    pub unknown_bool_2: bool,
    /// Unknown signed integer field 1.
    pub unknown_i32_1: i32,
    /// Unknown signed integer field 2.
    pub unknown_i32_2: i32,
    /// Unknown unsigned short field 2 (padded with `0xCDCD`).
    pub unknown_u16_2: u16,
    /// Unknown unsigned short field 3 (padded with `0xCDCD`).
    pub unknown_u16_3: u16,
    /// Unknown boolean flag 3.
    pub unknown_bool_3: bool,
    /// State-value pairs associated with this sell item.
    pub state_value_pairs: Vec<StateValuePair>,
    /// Unknown trailing string.
    pub unknown_string: String,
    /// The condition expression governing this item's availability.
    pub expression: SequenceExpression,
}

impl SellItem {
    /// Deserialise from the file stream.
    pub fn unpack(fm: &mut FileManipulator) -> Result<Self, SequenceFileError> {
        let item_name = fm.r_str_jps();
        let item_name2 = fm.r_str_jps();
        let unknown_u16_1 = fm.r_u16_jps();
        let unknown_bool_1 = fm.r_bool();
        let unknown_bool_2 = fm.r_bool();
        let unknown_i32_1 = fm.r_s32();
        let unknown_i32_2 = fm.r_s32();
        let unknown_u16_2 = fm.r_u16_jps();
        let unknown_u16_3 = fm.r_u16_jps();
        let unknown_bool_3 = fm.r_bool();
        let pair_count = fm.r_u32();
        let mut state_value_pairs = Vec::with_capacity(pair_count as usize);
        for _ in 0..pair_count {
            state_value_pairs.push(StateValuePair::unpack(fm));
        }
        let unknown_string = fm.r_str_jps();
        let expression = SequenceExpression::unpack(fm)?;
        Ok(Self {
            item_name,
            item_name2,
            unknown_u16_1,
            unknown_bool_1,
            unknown_bool_2,
            unknown_i32_1,
            unknown_i32_2,
            unknown_u16_2,
            unknown_u16_3,
            unknown_bool_3,
            state_value_pairs,
            unknown_string,
            expression,
        })
    }

    /// Serialise to bytes.
    pub fn pack(&self, endian: EndianType) -> Vec<u8> {
        let mut fm = new_fm(endian);
        fm.w_str_jps(&self.item_name);
        fm.w_str_jps(&self.item_name2);
        fm.w_u16_jps(self.unknown_u16_1, 0); // 0xCDCD
        fm.w_bool(self.unknown_bool_1);
        fm.w_bool(self.unknown_bool_2);
        fm.w_s32(self.unknown_i32_1);
        fm.w_s32(self.unknown_i32_2);
        fm.w_u16_jps(self.unknown_u16_2, 0); // 0xCDCD
        fm.w_u16_jps(self.unknown_u16_3, 0); // 0xCDCD
        fm.w_bool(self.unknown_bool_3);
        fm.w_u32(self.state_value_pairs.len() as u32);
        for pair in &self.state_value_pairs {
            fm.write(&pair.pack(endian));
        }
        fm.w_str_jps(&self.unknown_string);
        fm.write(&self.expression.pack(endian));
        fm.get_data().to_vec()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Events
// ─────────────────────────────────────────────────────────────────────────────

/// A single scripted event within a [`Sequence`].
///
/// Every event on disk begins with a JPS u16 type tag, followed by a common
/// `base_string` JPS string, then type-specific payload.
///
/// Type tag mapping (matches the C# `GetEventType` switch):
///
/// | Tag | Variant |
/// |-----|---------|
/// | 0   | `Dialog`        |
/// | 1   | `RandomDialog`  |
/// | 2   | `Choice`        |
/// | 3   | `Camera`        |
/// | 4   | `Branch`        |
/// | 5   | `Jump`          |
/// | 6   | `Inventory`     |
/// | 7   | `LuaCall`       |
/// | 8   | `End`           |
/// | 9   | `SetGlobal`     |
/// | 10  | `Delay`         |
/// | 11  | `Sell`          |
/// | 12  | `ChainSequence` |
/// | 15  | `Quest`         |
/// | 16  | `Text`          |
/// | 13/14 | deprecated (error on read) |
#[derive(Debug, Clone)]
pub enum SequenceEvent {
    /// Plays a single dialog line.
    Dialog {
        /// The event's context / actor string.
        base_string: String,
        /// The dialog line to play.
        dialog: DialogInfo,
    },
    /// Picks a random dialog line from a pool.
    RandomDialog {
        /// The event's context / actor string.
        base_string: String,
        /// Unknown unsigned short field.
        unknown_u16: u16,
        /// The pool of dialog lines.
        dialogs: Vec<DialogInfo>,
    },
    /// Presents a branching dialog choice to the player.
    Choice {
        /// The event's context / actor string.
        base_string: String,
        /// The initial dialog prompt.
        dialog: DialogInfo,
        /// An unknown trailing string.
        unknown_string: String,
        /// The selectable choice entries.
        choices: Vec<ChoiceEntry>,
        /// The condition that gates this choice.
        expression: SequenceExpression,
    },
    /// Triggers a camera action.
    Camera {
        /// The event's context / actor string.
        base_string: String,
        /// Unknown string field 1.
        unknown_string1: String,
        /// Unknown string field 2.
        unknown_string2: String,
        /// Unknown float field.
        unknown_float: f32,
        /// Unknown boolean flag 1.
        unknown_bool1: bool,
        /// Unknown boolean flag 2.
        unknown_bool2: bool,
    },
    /// Branches to one of several child sequences based on some condition.
    Branch {
        /// The event's context / actor string.
        base_string: String,
        /// The child sequences to branch into.
        sequences: Vec<Sequence>,
    },
    /// Jumps to a different sequence.
    Jump {
        /// The event's context / actor string.
        base_string: String,
        /// The target sequence name.
        target_sequence: String,
        /// An unknown secondary string.
        unknown_string2: String,
        /// Unknown boolean flag.
        unknown_bool: bool,
        /// The condition expression that triggers the jump.
        expression: SequenceExpression,
    },
    /// Modifies the player's inventory.
    Inventory {
        /// The event's context / actor string.
        base_string: String,
        /// Unknown unsigned short field.
        unknown_u16: u16,
        /// The item name string.
        item_string: String,
        /// Unknown signed integer.
        unknown_i32: i32,
        /// Unknown boolean flag.
        unknown_bool: bool,
    },
    /// Calls a Lua code snippet.
    LuaCall {
        /// The event's context / actor string.
        base_string: String,
        /// The Lua code snippet to execute.
        lua_snippet: String,
    },
    /// Marks the end of a sequence branch.
    End {
        /// The event's context / actor string.
        base_string: String,
        /// Unknown unsigned short field.
        unknown_u16: u16,
    },
    /// Sets one or more global state variables.
    SetGlobal {
        /// The event's context / actor string.
        base_string: String,
        /// Unknown boolean flag.
        unknown_bool: bool,
        /// The state-value pairs to apply.
        state_value_pairs: Vec<StateValuePair>,
    },
    /// Introduces a timed delay before the next event.
    Delay {
        /// The event's context / actor string.
        base_string: String,
        /// The delay duration in seconds.
        delay_seconds: f32,
    },
    /// Opens a shop/sell interface.
    Sell {
        /// The event's context / actor string.
        base_string: String,
        /// Unknown string field.
        unknown_string: String,
        /// Unknown unsigned short field (padded with `0xCDCD`).
        unknown_u16: u16,
        /// The items available for sale.
        sell_items: Vec<SellItem>,
    },
    /// Chains to another sequence file.
    ChainSequence {
        /// The event's context / actor string.
        base_string: String,
        /// The target sequence or file name.
        target: String,
        /// Unknown boolean flag.
        unknown_bool: bool,
    },
    /// Updates a quest state.
    ///
    /// Valid statuses: `"Discover"`, `"TurnIn"`, `"Complete"`, `"Unavailable"`.
    Quest {
        /// The event's context / actor string.
        base_string: String,
        /// The quest name.
        quest_name: String,
        /// A secondary quest identifier.
        quest_name2: String,
        /// The new quest status.
        status: String,
        /// Unknown boolean flag.
        unknown_bool: bool,
    },
    /// Displays text on-screen for a duration.
    Text {
        /// The event's context / actor string.
        base_string: String,
        /// The text localisation key.
        text_key: String,
        /// How long to display the text (seconds).
        display_time: f32,
    },
}

/// Valid quest statuses accepted by [`SequenceEvent::Quest`].
pub const VALID_QUEST_STATUSES: &[&str] = &["Discover", "TurnIn", "Complete", "Unavailable"];

impl SequenceEvent {
    /// Returns the binary type tag for this event variant.
    pub fn type_tag(&self) -> u16 {
        match self {
            Self::Dialog { .. } => 0,
            Self::RandomDialog { .. } => 1,
            Self::Choice { .. } => 2,
            Self::Camera { .. } => 3,
            Self::Branch { .. } => 4,
            Self::Jump { .. } => 5,
            Self::Inventory { .. } => 6,
            Self::LuaCall { .. } => 7,
            Self::End { .. } => 8,
            Self::SetGlobal { .. } => 9,
            Self::Delay { .. } => 10,
            Self::Sell { .. } => 11,
            Self::ChainSequence { .. } => 12,
            Self::Quest { .. } => 15,
            Self::Text { .. } => 16,
        }
    }

    /// Read a full event (type-tag + base_string + body) from the stream.
    pub fn unpack(fm: &mut FileManipulator) -> Result<Self, SequenceFileError> {
        let tag = fm.r_u16_jps();
        let base_string = fm.r_str_jps();
        Self::unpack_body(tag, base_string, fm)
    }

    fn unpack_body(
        tag: u16,
        base_string: String,
        fm: &mut FileManipulator,
    ) -> Result<Self, SequenceFileError> {
        match tag {
            0 => Ok(Self::Dialog {
                base_string,
                dialog: DialogInfo::unpack(fm),
            }),
            1 => {
                let unknown_u16 = fm.r_u16_jps();
                let dialog_count = fm.r_u32();
                let mut dialogs = Vec::with_capacity(dialog_count as usize);
                for _ in 0..dialog_count {
                    dialogs.push(DialogInfo::unpack(fm));
                }
                Ok(Self::RandomDialog {
                    base_string,
                    unknown_u16,
                    dialogs,
                })
            }
            2 => {
                let dialog = DialogInfo::unpack(fm);
                let unknown_string = fm.r_str_jps();
                let choice_count = fm.r_u32();
                let mut choices = Vec::with_capacity(choice_count as usize);
                for _ in 0..choice_count {
                    choices.push(ChoiceEntry::unpack(fm));
                }
                let expression = SequenceExpression::unpack(fm)?;
                Ok(Self::Choice {
                    base_string,
                    dialog,
                    unknown_string,
                    choices,
                    expression,
                })
            }
            3 => Ok(Self::Camera {
                base_string,
                unknown_string1: fm.r_str_jps(),
                unknown_string2: fm.r_str_jps(),
                unknown_float: fm.r_float(),
                unknown_bool1: fm.r_bool(),
                unknown_bool2: fm.r_bool(),
            }),
            4 => {
                let seq_count = fm.r_u16_jps() as u32;
                let mut sequences = Vec::with_capacity(seq_count as usize);
                for _ in 0..seq_count {
                    sequences.push(Sequence::unpack(fm)?);
                }
                Ok(Self::Branch {
                    base_string,
                    sequences,
                })
            }
            5 => {
                let target_sequence = fm.r_str_jps();
                let unknown_string2 = fm.r_str_jps();
                let unknown_bool = fm.r_bool();
                let expression = SequenceExpression::unpack(fm)?;
                Ok(Self::Jump {
                    base_string,
                    target_sequence,
                    unknown_string2,
                    unknown_bool,
                    expression,
                })
            }
            6 => Ok(Self::Inventory {
                base_string,
                unknown_u16: fm.r_u16_jps(),
                item_string: fm.r_str_jps(),
                unknown_i32: fm.r_s32(),
                unknown_bool: fm.r_bool(),
            }),
            7 => Ok(Self::LuaCall {
                base_string,
                lua_snippet: fm.r_str_jps(),
            }),
            8 => Ok(Self::End {
                base_string,
                unknown_u16: fm.r_u16_jps(),
            }),
            9 => {
                let unknown_bool = fm.r_bool();
                let pair_count = fm.r_u32();
                let mut pairs = Vec::with_capacity(pair_count as usize);
                for _ in 0..pair_count {
                    pairs.push(StateValuePair::unpack(fm));
                }
                Ok(Self::SetGlobal {
                    base_string,
                    unknown_bool,
                    state_value_pairs: pairs,
                })
            }
            10 => Ok(Self::Delay {
                base_string,
                delay_seconds: fm.r_float(),
            }),
            11 => {
                let unknown_string = fm.r_str_jps();
                let unknown_u16 = fm.r_u16_jps();
                let item_count = fm.r_u32();
                let mut sell_items = Vec::with_capacity(item_count as usize);
                for _ in 0..item_count {
                    sell_items.push(SellItem::unpack(fm)?);
                }
                Ok(Self::Sell {
                    base_string,
                    unknown_string,
                    unknown_u16,
                    sell_items,
                })
            }
            12 => Ok(Self::ChainSequence {
                base_string,
                target: fm.r_str_jps(),
                unknown_bool: fm.r_bool(),
            }),
            13 | 14 => Err(SequenceFileError::DeprecatedEventType(tag)),
            15 => {
                let quest_name = fm.r_str_jps();
                let quest_name2 = fm.r_str_jps();
                let status = fm.r_str_jps();
                if !VALID_QUEST_STATUSES.contains(&status.as_str()) {
                    return Err(SequenceFileError::InvalidEnumValue {
                        field: "quest_status",
                        value: status,
                    });
                }
                let unknown_bool = fm.r_bool();
                Ok(Self::Quest {
                    base_string,
                    quest_name,
                    quest_name2,
                    status,
                    unknown_bool,
                })
            }
            16 => Ok(Self::Text {
                base_string,
                text_key: fm.r_str_jps(),
                display_time: fm.r_float(),
            }),
            other => Err(SequenceFileError::UnknownEventType(other)),
        }
    }

    /// Serialise this event to bytes (type-tag + base_string + body).
    pub fn pack(&self, endian: EndianType) -> Vec<u8> {
        let mut fm = new_fm(endian);
        fm.w_u16_jps(self.type_tag(), 0); // 0xCDCD padding
        fm.write(&self.pack_body(endian));
        fm.get_data().to_vec()
    }

    fn pack_body(&self, endian: EndianType) -> Vec<u8> {
        let mut fm = new_fm(endian);
        match self {
            Self::Dialog {
                base_string,
                dialog,
            } => {
                fm.w_str_jps(base_string);
                fm.write(&dialog.pack(endian));
            }
            Self::RandomDialog {
                base_string,
                unknown_u16,
                dialogs,
            } => {
                fm.w_str_jps(base_string);
                fm.w_u16_jps(*unknown_u16, 0);
                fm.w_u32(dialogs.len() as u32);
                for d in dialogs {
                    fm.write(&d.pack(endian));
                }
            }
            Self::Choice {
                base_string,
                dialog,
                unknown_string,
                choices,
                expression,
            } => {
                fm.w_str_jps(base_string);
                fm.write(&dialog.pack(endian));
                fm.w_str_jps(unknown_string);
                fm.w_u32(choices.len() as u32);
                for c in choices {
                    fm.write(&c.pack(endian));
                }
                fm.write(&expression.pack(endian));
            }
            Self::Camera {
                base_string,
                unknown_string1,
                unknown_string2,
                unknown_float,
                unknown_bool1,
                unknown_bool2,
            } => {
                fm.w_str_jps(base_string);
                fm.w_str_jps(unknown_string1);
                fm.w_str_jps(unknown_string2);
                fm.w_float(*unknown_float);
                fm.w_bool(*unknown_bool1);
                fm.w_bool(*unknown_bool2);
            }
            Self::Branch {
                base_string,
                sequences,
            } => {
                fm.w_str_jps(base_string);
                fm.w_u16_jps(sequences.len() as u16, 0);
                for seq in sequences {
                    fm.write(&seq.pack(endian));
                }
            }
            Self::Jump {
                base_string,
                target_sequence,
                unknown_string2,
                unknown_bool,
                expression,
            } => {
                fm.w_str_jps(base_string);
                fm.w_str_jps(target_sequence);
                fm.w_str_jps(unknown_string2);
                fm.w_bool(*unknown_bool);
                fm.write(&expression.pack(endian));
            }
            Self::Inventory {
                base_string,
                unknown_u16,
                item_string,
                unknown_i32,
                unknown_bool,
            } => {
                fm.w_str_jps(base_string);
                fm.w_u16_jps(*unknown_u16, 0);
                fm.w_str_jps(item_string);
                fm.w_s32(*unknown_i32);
                fm.w_bool(*unknown_bool);
            }
            Self::LuaCall {
                base_string,
                lua_snippet,
            } => {
                fm.w_str_jps(base_string);
                fm.w_str_jps(lua_snippet);
            }
            Self::End {
                base_string,
                unknown_u16,
            } => {
                fm.w_str_jps(base_string);
                fm.w_u16_jps(*unknown_u16, 0);
            }
            Self::SetGlobal {
                base_string,
                unknown_bool,
                state_value_pairs,
            } => {
                fm.w_str_jps(base_string);
                fm.w_bool(*unknown_bool);
                fm.w_u32(state_value_pairs.len() as u32);
                for pair in state_value_pairs {
                    fm.write(&pair.pack(endian));
                }
            }
            Self::Delay {
                base_string,
                delay_seconds,
            } => {
                fm.w_str_jps(base_string);
                fm.w_float(*delay_seconds);
            }
            Self::Sell {
                base_string,
                unknown_string,
                unknown_u16,
                sell_items,
            } => {
                fm.w_str_jps(base_string);
                fm.w_str_jps(unknown_string);
                fm.w_u16_jps(*unknown_u16, 0); // 0xCDCD
                fm.w_u32(sell_items.len() as u32);
                for item in sell_items {
                    fm.write(&item.pack(endian));
                }
            }
            Self::ChainSequence {
                base_string,
                target,
                unknown_bool,
            } => {
                fm.w_str_jps(base_string);
                fm.w_str_jps(target);
                fm.w_bool(*unknown_bool);
            }
            Self::Quest {
                base_string,
                quest_name,
                quest_name2,
                status,
                unknown_bool,
            } => {
                fm.w_str_jps(base_string);
                fm.w_str_jps(quest_name);
                fm.w_str_jps(quest_name2);
                fm.w_str_jps(status);
                fm.w_bool(*unknown_bool);
            }
            Self::Text {
                base_string,
                text_key,
                display_time,
            } => {
                fm.w_str_jps(base_string);
                fm.w_str_jps(text_key);
                fm.w_float(*display_time);
            }
        }
        fm.get_data().to_vec()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Sequence
// ─────────────────────────────────────────────────────────────────────────────

/// A named scripted sequence containing a list of [`SequenceEvent`]s.
///
/// A `Sequence` is the core unit of the format. The top-level [`SequenceFile`]
/// wraps exactly one, but `Branch` events can embed child sequences
/// recursively.
#[derive(Debug, Clone)]
pub struct Sequence {
    /// The name of this sequence.
    pub name: String,
    /// The character/NPC this sequence is associated with.
    pub character: String,
    /// The area/zone this sequence belongs to.
    pub area: String,
    /// Sequence flags (typically `1`).
    pub flags: u16,
    /// Scheduling priority (typically `1`).
    pub priority: u16,
    /// The ordered list of events in this sequence.
    pub events: Vec<SequenceEvent>,
}

impl Sequence {
    /// Creates a new, empty sequence with sensible defaults.
    pub fn new(name: String, character: String, area: String) -> Self {
        Self {
            name,
            character,
            area,
            flags: 1,
            priority: 1,
            events: Vec::new(),
        }
    }

    /// Deserialise a sequence from the file stream.
    pub fn unpack(fm: &mut FileManipulator) -> Result<Self, SequenceFileError> {
        let name = fm.r_str_jps();
        let character = fm.r_str_jps();
        let area = fm.r_str_jps();
        let flags = fm.r_u16_jps();
        let priority = fm.r_u16_jps();
        let event_count = fm.r_u32();
        let mut events = Vec::with_capacity(event_count as usize);
        for _ in 0..event_count {
            events.push(SequenceEvent::unpack(fm)?);
        }
        Ok(Self {
            name,
            character,
            area,
            flags,
            priority,
            events,
        })
    }

    /// Serialise this sequence to bytes.
    ///
    /// A padding trailer (`00 00 CD CD 00 00 00 00`) is appended after the
    /// last event, matching the observed layout in original game assets.
    pub fn pack(&self, endian: EndianType) -> Vec<u8> {
        let mut fm = new_fm(endian);
        fm.w_str_jps(&self.name);
        fm.w_str_jps(&self.character);
        fm.w_str_jps(&self.area);
        fm.w_u16_jps(self.flags, 1); // 0xFFFF padding
        fm.w_u16_jps(self.priority, 1); // 0xFFFF padding
        fm.w_u32(self.events.len() as u32);
        for event in &self.events {
            fm.write(&event.pack(endian));
        }
        // 8-byte trailer observed in original assets after the last event.
        fm.w_u16(0x0000);
        fm.w_u16(0xCDCD);
        fm.w_u32(0x00000000);
        fm.get_data().to_vec()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SequenceFile
// ─────────────────────────────────────────────────────────────────────────────

/// The top-level container for a `.seq` sequence file.
///
/// A sequence file begins with a 4-byte little-endian version number (typically
/// `30`), followed immediately by the serialised [`Sequence`].
///
/// # Endianness
///
/// Wii files use **big-endian** byte order. The endianness can be detected
/// automatically with [`SequenceFile::from_binary_detect_endian`], which
/// mirrors the heuristic used by the C# reference: if `data[0] == 0` then
/// big-endian, otherwise little-endian.
///
/// # Examples
///
/// ```no_run
/// use epic_mickey_lib_rs::sequence_file::SequenceFile;
/// use epic_mickey_lib_rs::file_manipulator::EndianType;
///
/// // Load, inspect, re-save (preserving endianness).
/// let sf = SequenceFile::from_path("Neutral.bsq", EndianType::BIG).unwrap();
/// println!("version: {}", sf.version);
/// std::fs::write("Neutral_out.bsq", sf.to_binary(EndianType::BIG)).unwrap();
/// ```
#[derive(Debug, Clone)]
pub struct SequenceFile {
    /// File format version. Typically `30`.
    pub version: u32,
    /// The sequence data contained in this file.
    pub sequence: Sequence,
}

impl SequenceFile {
    /// Creates a new `SequenceFile` with the given version and sequence.
    pub fn new(version: u32, sequence: Sequence) -> Self {
        Self { version, sequence }
    }

    /// Deserialise from a byte slice with an explicit endian setting.
    pub fn from_binary(data: &[u8], endian: EndianType) -> Result<Self, SequenceFileError> {
        let mut fm = FileManipulator::new(data.to_vec(), endian, WriteMode::OVERWRITE);
        let version = fm.r_u32();
        let sequence = Sequence::unpack(&mut fm)?;
        Ok(Self { version, sequence })
    }

    /// Deserialise from a byte slice, auto-detecting endianness.
    ///
    /// If `data[0] == 0` the file is treated as big-endian (Wii),
    /// otherwise little-endian — matching the heuristic in the C# reference.
    pub fn from_binary_detect_endian(data: &[u8]) -> Result<Self, SequenceFileError> {
        let endian = if data[0] == 0 {
            EndianType::BIG
        } else {
            EndianType::LITTLE
        };
        Self::from_binary(data, endian)
    }

    /// Load from a file path with an explicit endian setting.
    pub fn from_path(path: &str, endian: EndianType) -> Result<Self, SequenceFileError> {
        let data = fs::read(path)?;
        Self::from_binary(&data, endian)
    }

    /// Load from a file path, auto-detecting endianness.
    pub fn from_path_detect_endian(path: &str) -> Result<Self, SequenceFileError> {
        let data = fs::read(path)?;
        Self::from_binary_detect_endian(&data)
    }

    /// Serialise this file to a byte vector.
    pub fn to_binary(&self, endian: EndianType) -> Vec<u8> {
        let mut fm = new_fm(endian);
        fm.w_u32(self.version);
        fm.write(&self.sequence.pack(endian));
        fm.get_data().to_vec()
    }

    /// Write the serialised file to the given path.
    pub fn to_path(&self, path: &str, endian: EndianType) -> Result<(), SequenceFileError> {
        fs::write(path, self.to_binary(endian))?;
        Ok(())
    }
}
