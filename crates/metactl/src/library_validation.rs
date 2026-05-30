use super::*;

pub(super) fn validate_skill_frontmatter_text(text: &str) -> Vec<String> {
    let mut failures = Vec::new();
    let mut lines = text.lines();
    if lines.next() != Some("---") {
        failures.push("SKILL.md must start with YAML frontmatter.".to_string());
        return failures;
    }

    let mut yaml_lines = Vec::new();
    let mut closed = false;
    for line in lines {
        if line.trim_end() == "---" {
            closed = true;
            break;
        }
        yaml_lines.push(line);
    }
    if !closed {
        failures.push("SKILL.md frontmatter is not closed.".to_string());
        return failures;
    }

    let yaml = yaml_lines.join("\n");
    let value: serde_yaml::Value = match serde_yaml::from_str(&yaml) {
        Ok(value) => value,
        Err(err) => {
            failures.push(format!("SKILL.md frontmatter invalid YAML: {err}."));
            return failures;
        }
    };

    for field in ["name", "description"] {
        match value.get(field).and_then(|item| item.as_str()) {
            Some(value) if !value.trim().is_empty() => {}
            _ => failures.push(format!("SKILL.md frontmatter.{field} is required.")),
        }
    }

    failures
}

pub(super) fn validate_generated_skill_frontmatter(path: &Path) -> Result<Vec<String>> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("read generated skill {}", path.display()))?;
    Ok(validate_skill_frontmatter_text(&text))
}

pub(super) fn validate_staged_outputs(
    project_root: PathBuf,
    manifest: &CompileManifest,
) -> Result<Vec<String>> {
    let mut failures = Vec::new();
    for output in &manifest.generated_outputs {
        let staged_abs = project_root.join(&output.path);
        if !staged_abs.exists() {
            failures.push(format!("{} is missing from the staging area.", output.path));
            continue;
        }
        if output.kind == GeneratedOutputKind::SkillFolder {
            for failure in validate_generated_skill_frontmatter(&staged_abs)? {
                failures.push(format!("{}: {}", output.path, failure));
            }
        }
        if let Some(expected) = output.digest.as_deref() {
            let actual = sha256_digest(&staged_abs)?;
            if actual != expected {
                failures.push(format!(
                    "{} digest mismatch: expected {} got {}.",
                    output.path, expected, actual
                ));
            }
        }
    }
    Ok(failures)
}
