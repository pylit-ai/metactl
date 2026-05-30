use super::*;

pub(super) fn render_frontmatter(frontmatter: &BTreeMap<String, String>) -> Result<String> {
    let typed_frontmatter = frontmatter
        .iter()
        .map(|(key, value)| (key.clone(), yaml_frontmatter_value(value)))
        .collect::<BTreeMap<_, _>>();
    let yaml = serde_yaml::to_string(&typed_frontmatter).context("serialize YAML frontmatter")?;
    let mut out = String::from("---\n");
    out.push_str(&yaml);
    if !yaml.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("---\n");
    Ok(out)
}

fn yaml_frontmatter_value(value: &str) -> serde_yaml::Value {
    match value {
        "true" => serde_yaml::Value::Bool(true),
        "false" => serde_yaml::Value::Bool(false),
        _ => serde_yaml::Value::String(value.to_string()),
    }
}

pub(super) fn wrap_with_frontmatter(
    body: &[u8],
    frontmatter: &BTreeMap<String, String>,
) -> Result<Vec<u8>> {
    if frontmatter.is_empty() {
        return Ok(body.to_vec());
    }
    let mut out = render_frontmatter(frontmatter)?;
    out.push_str(std::str::from_utf8(body).unwrap_or(""));
    Ok(out.into_bytes())
}
