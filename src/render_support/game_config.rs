use std::ops::Range;
fn tokens(text: &str) -> Vec<Range<usize>> {
    let mut result = Vec::new();
    let mut start = None;
    let mut quoted = false;
    for (i, c) in text.char_indices() {
        if c == '"' {
            quoted = !quoted;
        }
        if c.is_whitespace() && !quoted {
            if let Some(a) = start.take() {
                result.push(a..i);
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(a) = start {
        result.push(a..text.len());
    }
    result
}
pub fn launch_level(text: &str) -> anyhow::Result<String> {
    let first = tokens(text)
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("cmdline.txt has no launch level"))?;
    let level = text[first].trim_matches('"');
    set_launch_level(text, level)?;
    Ok(level.into())
}
pub fn set_launch_level(text: &str, level: &str) -> anyhow::Result<String> {
    crate::mod_project::valid_path(level)?;
    anyhow::ensure!(
        [".level", ".bin"]
            .iter()
            .any(|ext| level.to_ascii_lowercase().ends_with(ext))
            && !level.contains(['"', '\r', '\n']),
        "Choose a .level or .bin asset"
    );
    let first = tokens(text)
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("cmdline.txt has no launch level"))?;
    anyhow::ensure!(
        [".level", ".bin"].iter().any(|ext| text[first.clone()]
            .trim_matches('"')
            .to_ascii_lowercase()
            .ends_with(ext)),
        "The command line does not start with a recognized level path"
    );
    let mut result = text.to_string();
    result.replace_range(
        first,
        &if level.contains(char::is_whitespace) {
            format!("\"{level}\"")
        } else {
            level.into()
        },
    );
    Ok(result)
}
/// Set a launch override while retaining unrelated arguments and repeated keys.
fn set_launch_option(text: &str, name: &str, value: &str) -> String {
    let rows: Vec<_> = entries(text, true)
        .into_iter()
        .filter(|e| e.name == name)
        .collect();
    if rows.is_empty() {
        return format!("{} -Set {name}={value}", text.trim_end());
    }
    let mut result = text.to_string();
    for row in rows.into_iter().rev() {
        result.replace_range(row.span, value);
    }
    result
}

#[derive(Clone, Debug)]
struct SettingEntry {
    name: String,
    section: String,
    span: Range<usize>,
    identity: String,
}
fn entries(text: &str, command_line: bool) -> Vec<SettingEntry> {
    let mut rows = Vec::new();
    let mut occurrences = std::collections::BTreeMap::new();
    let mut add = |section: &str, name: String, span| {
        let occurrence = occurrences
            .entry((section.to_string(), name.clone()))
            .or_insert(0usize);
        let identity = format!(
            "@entry:{}",
            serde_json::to_string(&(command_line, section, &name, *occurrence)).unwrap()
        );
        *occurrence += 1;
        rows.push(SettingEntry {
            name,
            section: section.into(),
            span,
            identity,
        });
    };
    if command_line {
        let spans = tokens(text);
        for pair in spans.windows(2) {
            if &text[pair[0].clone()] == "-Set" {
                let span = &pair[1];
                if let Some(eq) = text[span.clone()].find('=') {
                    add(
                        "",
                        text[span.start..span.start + eq].into(),
                        span.start + eq + 1..span.end,
                    );
                }
            }
        }
    } else {
        let mut offset = 0;
        let mut section = String::new();
        for line in text.split_inclusive('\n') {
            let trimmed = line.trim();
            if let Some(header) = trimmed.strip_prefix('[') {
                if let Some(end) = header.find(']') {
                    section = header[..end].trim().into();
                }
            } else if !trimmed.starts_with([';', '#']) && !trimmed.starts_with("//") {
                if let Some(eq) = line.find('=') {
                    let content = line.trim_end_matches(['\r', '\n']);
                    let value = &content[eq + 1..];
                    let leading = value.len() - value.trim_start().len();
                    let end = value.trim_end().len().max(leading);
                    if end >= leading {
                        add(
                            &section,
                            line[..eq].trim().into(),
                            offset + eq + 1 + leading..offset + eq + 1 + end,
                        );
                    }
                }
            }
            offset += line.len();
        }
    }
    rows
}
pub fn setting_overrides(
    base: &str,
    edited: &str,
    command: bool,
) -> std::collections::BTreeMap<String, String> {
    let old: std::collections::BTreeMap<_, _> = entries(base, command)
        .into_iter()
        .map(|e| (e.identity, base[e.span].to_string()))
        .collect();
    let mut result: std::collections::BTreeMap<_, _> = entries(edited, command)
        .into_iter()
        .filter_map(|e| {
            let value = &edited[e.span];
            (old.get(&e.identity).map(String::as_str) != Some(value))
                .then(|| (e.identity, value.to_string()))
        })
        .collect();
    if command {
        if let (Some(a), Some(b)) = (tokens(base).first(), tokens(edited).first()) {
            if base[a.clone()] != edited[b.clone()] {
                result.insert("@launch".into(), edited[b.clone()].trim_matches('"').into());
            }
        }
    }
    result
}
/// Read a setting using the same syntax as the configuration editor.
pub fn setting_value(text: &str, name: &str, command: bool) -> Option<String> {
    entries(text, command)
        .into_iter()
        .rev()
        .find(|entry| entry.name.eq_ignore_ascii_case(name))
        .map(|entry| text[entry.span].trim().trim_matches('"').to_owned())
}

/// Runtime names include the INI section (for example MickeyAbilities_Spin_Paint_Wait_1).
/// Command-line -Set names are already qualified. Later entries win.
pub fn runtime_settings(text: &str, command: bool) -> std::collections::BTreeMap<String, String> {
    entries(text, command)
        .into_iter()
        .map(|entry| {
            let name = if entry.section.is_empty() {
                entry.name
            } else {
                format!("{}_{}", entry.section, entry.name)
            };
            (name, text[entry.span].trim().trim_matches('"').to_owned())
        })
        .collect()
}
/// Old releases stored only a bare name. Never guess which repeated setting it meant.
pub fn ambiguous_overrides(
    source: &str,
    values: &std::collections::BTreeMap<String, String>,
    command: bool,
) -> Vec<String> {
    ambiguous_entries(&entries(source, command), values)
}
fn ambiguous_entries(
    rows: &[SettingEntry],
    values: &std::collections::BTreeMap<String, String>,
) -> Vec<String> {
    let mut counts = std::collections::BTreeMap::new();
    for e in rows {
        *counts.entry(e.name.clone()).or_insert(0usize) += 1;
    }
    counts
        .into_iter()
        .filter(|(name, count)| *count > 1 && values.contains_key(name))
        .map(|(name, _)| name)
        .collect()
}
pub fn apply_overrides(
    source: &str,
    values: &std::collections::BTreeMap<String, String>,
    command: bool,
) -> anyhow::Result<String> {
    if values.is_empty() {
        return Ok(source.to_string());
    }
    let rows = entries(source, command);
    let ambiguous = ambiguous_entries(&rows, values);
    anyhow::ensure!(ambiguous.is_empty(), "{} saved settings are ambiguous ({}{}). Open User game settings, choose the intended sections, then save or reset the settings.", ambiguous.len(), ambiguous.iter().take(6).cloned().collect::<Vec<_>>().join(", "), if ambiguous.len()>6 { ", …" } else { "" });
    let mut text = source.to_string();
    for e in rows.into_iter().rev() {
        if let Some(value) = values.get(&e.identity).or_else(|| values.get(&e.name)) {
            text.replace_range(e.span, value);
        }
    }
    if command {
        let existing: std::collections::HashSet<_> = entries(source, true)
            .into_iter()
            .map(|row| row.identity)
            .collect();
        for (identity, value) in values {
            if existing.contains(identity) {
                continue;
            }
            let Some(encoded) = identity.strip_prefix("@entry:") else {
                continue;
            };
            let Ok((is_command, section, name, occurrence)) =
                serde_json::from_str::<(bool, String, String, usize)>(encoded)
            else {
                continue;
            };
            if is_command && section.is_empty() && occurrence == 0 {
                anyhow::ensure!(
                    !name.is_empty()
                        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                        && !value.is_empty()
                        && !value.contains(['\r', '\n', '"'])
                        && !value.chars().any(char::is_whitespace),
                    "Invalid new launch setting: {name}"
                );
                text = set_launch_option(&text, &name, value);
            }
        }
        if let Some(level) = values.get("@launch") {
            text = set_launch_level(&text, level)?;
        }
    }
    Ok(text)
}
