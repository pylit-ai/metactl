# metactl Operations

Use these operations when applying library-organization decisions in a real workspace.

## Inspect First

- `metactl status`
- `metactl list packs`
- `metactl search <topic>`
- `metactl explain --json`

## Choose the Right Layer

- User-global reusable default:
  Put shared library roots and default packs in `~/.config/metactl/profiles/<name>.yaml`.
- Shared project baseline:
  Store repo-wide targets or pack choices in `metactl.yaml`.
- Shared profile binding:
  Use `metactl init --bind-profile` when the repo should record `extends_profile: <name>`.
- Personal-only additions:
  Use `metactl use --local <pack>` so the change lands in `metactl.local.yaml`.
- Fleet controller:
  Store the controller project at `/path/to/metactl-library/fleet/<name>`, `~/.config/metactl/fleet/<name>`, or a dedicated private repo. Create it with `metactl fleet controller init <name>` or `metactl fleet controller init <name> --path /path/to/controller`. Register an existing controller with `metactl fleet controller set <name> /path/to/controller`.

## File Triage

- If the change should follow you across many repos:
  edit `~/.config/metactl/profiles/<name>.yaml`.
- If the change is part of the repo's shared contract:
  edit `metactl.yaml`.
- If the change should stay private to your machine:
  use `metactl.local.yaml` or `metactl use --local`.
- If the change is a reusable public capability:
  edit or add a pack in the library root, not in a generated target surface.
- If the change is a multi-repo Fleet registry:
  edit the controller project's `metactl.yaml`, not a pack manifest and not the user-global settings file.

## Visibility Checks

- Use `metactl explain --json` to inspect selected and suppressed packs.
- Keep private or user-specific material out of public starter packs.
- Treat `metactl.local.yaml` as the default home for personal-only add-ons.
- Treat `linked_projects` as controller-project state. The user-global config should store only the selected Fleet controller pointer.
- If a pack exists only to wrap another capability for one runtime, keep it out of the smallest shared defaults unless there is a clear cross-project reason.

## Profile Patterns

- `catalog`: library root only, no default packs
- `generalist`: small baseline used across most projects
- `research` / `authoring` / `verification`: specialist profiles for higher-context work
- `full`: explicit everything-enabled profile for audits or dogfooding, not the default

## Update Flow

1. Edit the pack manifest or profile YAML at the source of truth.
2. Re-check with `metactl status` or `metactl list packs`.
3. For Fleet controller changes, run `metactl fleet sync --preview` before any apply.
4. If project artifacts should refresh automatically after config changes, install repo hooks with `metactl hook install`.
5. Keep public starter packs free of private or user-specific content.
