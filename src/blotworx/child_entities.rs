//! ChildComponent named lookup, REL311f5c..3120b4. This list is distinct
//! from transform descendants and prefab membership. Its authored order and
//! null/stale slots survive lookup; the first matching live child wins.
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::scene_file::Component;
pub fn requested_name(name: &str) -> Result<&str> {
    let name = name.split('\0').next().unwrap_or("");
    if !name.starts_with('[') {
        return Ok(name);
    }
    let (_, tail) = name
        .split_once(']')
        .context("Malformed decorated child name: missing closing bracket")?;
    Ok(tail.rsplit_once('.').map_or(tail, |(_, name)| name))
}
pub fn find<T>(
    component: Option<&Component>,
    name: &str,
    mut resolve: impl FnMut(u32) -> Result<Option<(T, String)>>,
) -> Result<Option<T>> {
    let name = requested_name(name)?;
    if name.is_empty() {
        return Ok(None);
    }
    let Some(component) = component else {
        return Ok(None);
    };
    ensure!(
        component.class_name == "ChildComponent",
        "Child lookup has the wrong component"
    );
    let Some(property) = component
        .properties
        .iter()
        .find(|p| p.name == "Child Entities")
    else {
        return Ok(None);
    };
    for value in property
        .value
        .as_array()
        .context("Invalid Child Entities array")?
    {
        let id = u32::try_from(value.as_u64().context("Invalid child entity handle")?)?;
        if id == 0 {
            continue;
        }
        if let Some((child, actual)) = resolve(id)? {
            if actual.eq_ignore_ascii_case(name) {
                return Ok(Some(child));
            }
        }
    }
    Ok(None)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decoration_is_request_only_and_live_list_order_controls_duplicate_names() -> Result<()> {
        use epic_mickey_lib_rs::scene_file::{SceneFile, SceneFileVersion, ID};
        let scene = SceneFile::new(
            vec![],
            vec![],
            vec![],
            ID::new(0),
            SceneFileVersion::Version1,
        );
        let mut component = crate::component_schema::CATALOG.components["ChildComponent"]
            .instantiate("ChildComponent", &scene)?;
        component
            .properties
            .iter_mut()
            .find(|p| p.name == "Child Entities")
            .unwrap()
            .value = serde_json::json!([0, 5, 9, 7]);
        let mut visited = Vec::new();
        let found = find(
            Some(&component),
            "[owner]namespace.other.GremlinWrench",
            |id| {
                visited.push(id);
                Ok((id != 5).then(|| (id, "gremlinwrench".into())))
            },
        )?;
        assert_eq!(found, Some(9));
        assert_eq!(visited, [5, 9]);
        assert_eq!(
            find(Some(&component), "wrench", |id| Ok(Some((
                id,
                "[owner]wrench".into()
            ))))?,
            None
        );
        assert_eq!(
            find::<u32>(None, "GremlinWrench", |_| anyhow::bail!(
                "Unexpected resolution"
            ))?,
            None
        );
        assert_eq!(
            requested_name("[owner]GremlinWrench\0ignored")?,
            "GremlinWrench"
        );
        assert!(requested_name("[missing").is_err());
        Ok(())
    }
}
