use super::*;

pub(super) fn cmd_profile(
    cli: &Cli,
    args: &ProfileArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = cli.project.as_deref();
    match &args.command {
        Some(ProfileCommand::List) => {
            let items = list_user_profiles().map_err(internal_error)?;
            let templates = builtin_profile_template_json();
            let profiles_dir = profiles_directory();
            let mut human = String::from("Profiles:\n");
            if items.is_empty() {
                human.push_str("  (none)\n");
            } else {
                for (name, path) in &items {
                    human.push_str(&format!("  {} — {}\n", name, path.display()));
                }
            }
            human.push_str(&format!(
                "Profiles directory: {}\n",
                profiles_dir
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "(unavailable — set HOME or XDG_CONFIG_HOME)".to_string())
            ));
            human.push_str("Built-in templates:\n");
            for template in &templates {
                human.push_str(&format!(
                    "  {} — {}\n",
                    template["name"].as_str().unwrap_or("?"),
                    template["description"].as_str().unwrap_or("")
                ));
            }
            let json_profiles: Vec<Value> = items
                .iter()
                .map(|(name, path)| {
                    json!({
                        "name": name,
                        "path": path,
                    })
                })
                .collect();
            Ok(CommandOutput {
                human,
                json: success_json(
                    "profile",
                    project_root,
                    json!({
                        "action": "list",
                        "profiles_directory": profiles_dir,
                        "profiles": json_profiles,
                        "templates": templates,
                    }),
                ),
            })
        }
        Some(ProfileCommand::Show) | None => {
            let settings = load_user_settings();
            let path = user_settings_path();
            let human = format!(
                "User settings file: {}\nDefault profile: {}\nProfiles directory: {}\n",
                path.as_ref()
                    .map(|item| item.display().to_string())
                    .unwrap_or_else(|| "(unavailable — set HOME or XDG_CONFIG_HOME)".to_string()),
                settings.default_profile.as_deref().unwrap_or("(none)"),
                profiles_directory()
                    .as_ref()
                    .map(|item| item.display().to_string())
                    .unwrap_or_else(|| "(unavailable — set HOME or XDG_CONFIG_HOME)".to_string()),
            );
            Ok(CommandOutput {
                human,
                json: success_json(
                    "profile",
                    project_root,
                    json!({
                        "action": "show",
                        "settings_path": path,
                        "default_profile": settings.default_profile,
                        "profiles_directory": profiles_directory(),
                        "templates": builtin_profile_template_json(),
                    }),
                ),
            })
        }
        Some(ProfileCommand::SetDefault { name }) => {
            let Some(profile_file) = profile_path(name) else {
                return Err(CliError::new(
                    EXIT_STATE,
                    "HOME (or XDG_CONFIG_HOME) is not set; cannot resolve profile path.",
                ));
            };
            let builtin = builtin_profile_templates()
                .iter()
                .any(|template| template.name == name.as_str());
            if !profile_file.exists() && !builtin {
                return Err(CliError::new(
                    EXIT_STATE,
                    format!(
                        "Profile file not found: {}.\nHint: create {}",
                        profile_file.display(),
                        profile_file.display()
                    ),
                ));
            }
            let mut settings = load_user_settings();
            settings.default_profile = Some(name.clone());
            save_user_settings(&settings).map_err(internal_error)?;
            let human = format!("Default profile set to `{name}`.\n");
            Ok(CommandOutput {
                human,
                json: success_json(
                    "profile",
                    project_root,
                    json!({
                        "action": "set-default",
                        "default_profile": name,
                    }),
                ),
            })
        }
        Some(ProfileCommand::ClearDefault) => {
            let mut settings = load_user_settings();
            settings.default_profile = None;
            save_user_settings(&settings).map_err(internal_error)?;
            Ok(CommandOutput {
                human: "Cleared default profile.\n".to_string(),
                json: success_json(
                    "profile",
                    project_root,
                    json!({
                        "action": "clear-default",
                        "default_profile": Value::Null,
                    }),
                ),
            })
        }
    }
}
