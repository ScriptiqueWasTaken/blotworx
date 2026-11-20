//! DOL80024af8 /80011838 narrow-string locale comparisons.
use std::cmp::Ordering;

/// Initial DOL locale8069f7c8 ->8069f5c8 +10 ->8064be20.
/// Only indices0..127 are reachable after the native signed-byte range test.
pub fn initial_table() -> [u8; 128] {
    std::array::from_fn(|i| {
        if (65..=90).contains(&i) {
            (i + 32) as u8
        } else {
            i as u8
        }
    })
}
fn folded(byte: u8, table: &[u8; 128]) -> i8 {
    if byte < 128 {
        table[byte as usize] as i8
    } else {
        byte as i8
    }
}
/// Includes C NUL semantics. Neither Unicode folding nor unsigned byte order
/// matches the original. Table is supplied by the runtime locale owner.
pub fn compare(a: &[u8], b: &[u8], table: &[u8; 128]) -> Ordering {
    compare_count(a, b, table, None)
}
/// DOL80011838 treats the count as signed: count<=0 compares equal.
pub fn compare_prefix(a: &[u8], b: &[u8], count: i32, table: &[u8; 128]) -> Ordering {
    if count <= 0 {
        return Ordering::Equal;
    }
    compare_count(a, b, table, Some(count as usize))
}
fn compare_count(a: &[u8], b: &[u8], table: &[u8; 128], limit: Option<usize>) -> Ordering {
    let mut index = 0;
    loop {
        if limit == Some(index) {
            return Ordering::Equal;
        }
        let first = folded(a.get(index).copied().unwrap_or(0), table);
        let second = folded(b.get(index).copied().unwrap_or(0), table);
        match first.cmp(&second) {
            Ordering::Equal if first != 0 => index += 1,
            result => return result,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn comparisons_preserve_native_signed_bytes_and_nul() {
        let table = initial_table();
        assert_eq!(compare(b"BigBall", b"bigball", &table), Ordering::Equal);
        assert_eq!(compare(b"None\0extra", b"NONE", &table), Ordering::Equal);
        assert_eq!(compare(&[0x80], b"a", &table), Ordering::Less);
        assert_eq!(compare(&[0xff], &[0x80], &table), Ordering::Greater);
        assert_eq!(
            compare_prefix(b"sketchAnvil", b"Sketch", 6, &table),
            Ordering::Equal
        );
        assert_eq!(compare_prefix(b"a", b"b", -1, &table), Ordering::Equal);
        assert_eq!(compare_prefix(b"a", b"b", 0, &table), Ordering::Equal);
        assert_eq!(compare(b"Sketch", b"SketchAnvil", &table), Ordering::Less);
        let mut custom = table;
        custom[b'!' as usize] = b'a';
        assert_eq!(compare(b"!", b"A", &custom), Ordering::Equal);
    }
    #[test]
    fn initial_table_matches_reachable_original_locale_entries() {
        let table = initial_table();
        for i in 0..128 {
            let expected = if (65..=90).contains(&i) { i + 32 } else { i };
            assert_eq!(table[i], expected as u8);
        }
    }
}
