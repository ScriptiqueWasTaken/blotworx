//! Portable, readable Lua changes against a versioned source baseline.
use super::{digest, Change};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TextEdit {
    /// UTF-8 byte offset in the original source, never in compiled bytecode.
    pub offset: usize,
    pub expected: String,
    pub replacement: String,
}
pub const NATIVE: &str = "unluac-1.4.3-lua51-v1";
pub const UTF8: &str = "utf8-v1";
pub const REPLACEMENT: &str = "replacement-v1";

pub fn baseline(data: &[u8], version: &str) -> Result<String> {
    match version {
        NATIVE => crate::lua_cook::native_source(data),
        UTF8 => Ok(std::str::from_utf8(data)?.into()),
        // Explicit source imports can replace chunks we cannot reconstruct.
        REPLACEMENT => Ok(String::new()),
        v if v.starts_with("reviewed-v1:") => {
            let entry = crate::lua_cook::repository::lookup_version(data, v)
                .context("No reviewed source for this original script revision")?;
            ensure!(
                crate::lua_cook::repository::version(entry) == v,
                "This mod needs another reviewed source revision"
            );
            Ok(entry.source.into())
        }
        _ => anyhow::bail!("Unsupported Lua source baseline: {version}"),
    }
}
pub fn supported(version: &str) -> bool {
    [NATIVE, UTF8, REPLACEMENT].contains(&version)
        || version
            .strip_prefix("reviewed-v1:")
            .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
}
pub fn capture(before: &[u8], after: &str) -> Result<Change> {
    crate::lua_cook::compile(after).context("Script changes contain invalid Lua 5.1")?;
    let reviewed = crate::lua_cook::repository::lookup(before);
    let reviewed_version = reviewed.map(crate::lua_cook::repository::version);
    let (version, base) = if let Some(entry) = reviewed {
        (
            reviewed_version.as_deref().unwrap(),
            entry.source.to_string(),
        )
    } else if before.starts_with(b"\x1bLua") {
        match baseline(before, NATIVE) {
            Ok(s) => (NATIVE, s),
            Err(_) => (REPLACEMENT, String::new()),
        }
    } else {
        (UTF8, baseline(before, UTF8)?)
    };
    Ok(Change::LuaSource {
        base_sha256: digest(before),
        source_sha256: digest(base.as_bytes()),
        source_version: version.into(),
        edits: if version == REPLACEMENT {
            vec![TextEdit {
                offset: 0,
                expected: String::new(),
                replacement: after.into(),
            }]
        } else {
            diff(&base, after)
        },
    })
}
pub fn validate(edits: &[TextEdit]) -> Result<()> {
    let mut end = 0;
    let mut previous = None;
    for e in edits {
        ensure!(
            e.offset >= end && previous != Some(e.offset),
            "Overlapping Lua source edits"
        );
        ensure!(
            e.expected != e.replacement || e.expected.is_empty(),
            "Empty Lua source edit"
        );
        end = e
            .offset
            .checked_add(e.expected.len())
            .context("Lua source range overflow")?;
        previous = Some(e.offset);
    }
    Ok(())
}
pub fn overlap(a: &TextEdit, b: &TextEdit) -> bool {
    a.offset < b.offset.saturating_add(b.expected.len().max(1))
        && b.offset < a.offset.saturating_add(a.expected.len().max(1))
}
pub fn apply(base: &str, edits: &[TextEdit]) -> Result<String> {
    validate(edits)?;
    // Check all ranges against the immutable baseline before changing anything.
    for e in edits {
        ensure!(
            base.get(e.offset..e.offset + e.expected.len()) == Some(e.expected.as_str()),
            "Lua source baseline changed near byte {}",
            e.offset
        );
    }
    let mut out = base.to_string();
    for e in edits.iter().rev() {
        out.replace_range(e.offset..e.offset + e.expected.len(), &e.replacement);
    }
    Ok(out)
}
pub fn preview(original: &[u8], change: &Change) -> Result<Vec<u8>> {
    let Change::LuaSource {
        base_sha256,
        source_sha256,
        source_version,
        edits,
    } = change
    else {
        anyhow::bail!("Expected a Lua source change");
    };
    ensure!(
        digest(original) == *base_sha256,
        "Wrong original Lua revision"
    );
    let base = baseline(original, source_version)?;
    ensure!(
        digest(base.as_bytes()) == *source_sha256,
        "Lua reconstruction differs from the mod's source baseline"
    );
    if edits.is_empty() {
        return Ok(original.to_vec());
    }
    Ok(apply(&base, edits)?.into_bytes())
}

/// Line hunks with a bounded LCS table. Large replacements fall back to one
/// middle hunk after trimming unchanged edges, keeping memory below 4 MiB.
pub fn diff(before: &str, after: &str) -> Vec<TextEdit> {
    let a: Vec<_> = before.split_inclusive('\n').collect();
    let b: Vec<_> = after.split_inclusive('\n').collect();
    let prefix = a.iter().zip(&b).take_while(|(a, b)| a == b).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let offset: usize = a[..prefix].iter().map(|s| s.len()).sum();
    let a = &a[prefix..a.len() - suffix];
    let b = &b[prefix..b.len() - suffix];
    if a.is_empty() && b.is_empty() {
        return vec![];
    }
    let width = b.len() + 1;
    if (a.len() + 1).saturating_mul(width) > 1_000_000 {
        return vec![TextEdit {
            offset,
            expected: a.concat(),
            replacement: b.concat(),
        }];
    }
    let mut table = vec![0u32; (a.len() + 1) * width];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            table[i * width + j] = if a[i] == b[j] {
                1 + table[(i + 1) * width + j + 1]
            } else {
                table[(i + 1) * width + j].max(table[i * width + j + 1])
            };
        }
    }
    let (mut i, mut j, mut pos) = (0, 0, offset);
    let mut result = vec![];
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && a[i] == b[j] {
            pos += a[i].len();
            i += 1;
            j += 1;
            continue;
        }
        let mut edit = TextEdit {
            offset: pos,
            expected: String::new(),
            replacement: String::new(),
        };
        while i < a.len() || j < b.len() {
            if i < a.len() && j < b.len() && a[i] == b[j] {
                break;
            }
            if j < b.len()
                && (i == a.len() || table[i * width + j + 1] >= table[(i + 1) * width + j])
            {
                edit.replacement.push_str(b[j]);
                j += 1;
            } else {
                edit.expected.push_str(a[i]);
                pos += a[i].len();
                i += 1;
            }
        }
        result.push(edit);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_hunks_preserve_unicode_crlf_and_independent_changes() {
        let base = "-- café\r\nlocal speed = 1\r\n\r\nlocal health = 2\r\nreturn speed + health";
        let after = base
            .replace("speed = 1", "speed = 100")
            .replace("health = 2", "health = 200");
        let edits = diff(base, &after);
        assert_eq!(edits.len(), 2);
        assert_eq!(apply(base, &edits).unwrap(), after);
        let json = serde_json::to_string(&edits).unwrap();
        assert!(json.contains("local speed = 100"));
        assert!(apply("wrong baseline", &edits).is_err());
        for a in ["", "a", "é\n", "a\nb\na\n", "x\r\ny"] {
            for b in ["", "b", "é\n", "a\na\n", "x\r\nz\n"] {
                assert_eq!(apply(a, &diff(a, b)).unwrap(), b);
            }
        }
    }
    #[test]
    fn invalid_ranges_and_large_diffs_are_bounded() {
        assert!(apply(
            "é",
            &[TextEdit {
                offset: 1,
                expected: String::new(),
                replacement: "x".into()
            }]
        )
        .is_err());
        assert!(validate(&[TextEdit {
            offset: usize::MAX,
            expected: "x".into(),
            replacement: "y".into()
        }])
        .is_err());
        let before = "a\n".repeat(2000);
        let after = "b\n".repeat(2000);
        let edits = diff(&before, &after);
        assert_eq!(edits.len(), 1);
        assert_eq!(apply(&before, &edits).unwrap(), after);
    }
}
