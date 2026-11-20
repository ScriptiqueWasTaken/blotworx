//! EM2 JPSEntityTemplate::Create startup flags. These belong to each template
//! instance; FixMasterTemplates inherits components, not this flags word.
//! Source: supplied Wii U CreateFromMemory (4493975), Create (4494387),
//! NiEntityInterface::GetStartHidden (230203), NotifySceneAddedToWorld (4408173).
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct State {
    pub source_flags: u32,
    pub persistent_components: u32,
    pub persists: bool,
    pub start_hidden: bool,
    pub persist_hidden: bool,
    pub persist_destroyed: bool,
    pub ignore_preserved_properties: bool,
}
impl State {
    pub fn decode(flags: u32, persistent_components: u32) -> Self {
        Self {
            source_flags: flags,
            persistent_components,
            persists: flags & 0x10 != 0,
            start_hidden: flags & 0x20 != 0,
            persist_hidden: flags & 0x40 != 0,
            persist_destroyed: flags & 0x80 != 0,
            ignore_preserved_properties: flags & 0x100 != 0,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_bits_are_independent_and_other_bits_are_retained() {
        for bit in 0..32 {
            let value = State::decode(1 << bit, 0xa55ac33c);
            assert_eq!(value.source_flags, 1 << bit);
            assert_eq!(value.persistent_components, 0xa55ac33c);
            assert_eq!(
                [
                    value.persists,
                    value.start_hidden,
                    value.persist_hidden,
                    value.persist_destroyed,
                    value.ignore_preserved_properties
                ],
                [bit == 4, bit == 5, bit == 6, bit == 7, bit == 8]
            );
        }
    }
}
