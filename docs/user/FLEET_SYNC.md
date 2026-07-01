# Fleet Sync

Fleet Sync previews or applies `metactl sync` across explicitly linked local projects. It stays local-first: there is no hosted control plane, no filesystem auto-discovery, and no background mutation.

## Controller Model

A Fleet controller is a normal metactl project whose `metactl.yaml` contains the canonical `linked_projects` registry. The machine-local user config may store a pointer to that controller so Fleet commands work from any directory.

Minimum controller contents:

```text
fleet/personal/
└── metactl.yaml
```

That can be only the config file. Add a short `README.md` when the registry is shared with teammates or agents, and let metactl create `.metactl/logs/` when apply runs. Do not put the controller under a pack directory; it is a project registry, not a pack.

Recommended locations:

| Use case | Location | Why |
| --- | --- | --- |
| Private, reviewable Fleet for a metactl library | `/path/to/metactl-library/fleet/<name>` | Best fit when the linked projects are part of the same private metactl operating context. Keeps registry near packs without mixing it into `packs/`. |
| Team-shared Fleet registry | a dedicated private repo such as `/path/to/team-metactl-fleet` | Clean ownership and review history. |
| Single-machine personal registry with no review needs | `~/.config/metactl/fleet/personal` | Correct config location, but less visible and easier to forget. |
| Ordinary development parent such as `/path/to/source-checkouts/metactl-fleet` | possible, but not preferred | Source checkout parents are usually for working repos, not controller config. |

For a private metactl library checkout, set a default controller:

```bash
metactl fleet controller init personal --path /path/to/metactl-library/fleet/personal
metactl fleet controller show
```

> **Expected output**
>
> ```text
> Fleet controller `personal` initialized at /path/to/metactl-library/fleet/personal.
> Next: edit /path/to/metactl-library/fleet/personal/metactl.yaml and add linked_projects, then run `metactl fleet sync --preview`.
>
> Fleet controller: personal
> Controller source: user_default
> Controller path: /path/to/metactl-library/fleet/personal
> ```

For a new single-machine setup, omit `--path` and metactl creates `~/.config/metactl/fleet/<name>`:

```bash
metactl fleet controller init personal
# Fleet controller `personal` initialized at ~/.config/metactl/fleet/personal.
# Next: edit ~/.config/metactl/fleet/personal/metactl.yaml and add linked_projects, then run `metactl fleet sync --preview`.
```

Use `set` only when the controller project already exists:

```bash
metactl fleet controller set personal /path/to/controller
# Fleet controller `personal` set to /path/to/controller.
```

Controller config:

```yaml
api_version: metactl/v2alpha1
role: builder
policy: brownfield-safe-builder
targets:
  - codex-cli
linked_projects:
  - id: metactl
    path: /path/to/repos/metactl
  - id: app
    path: /path/to/repos/app
```

## Importing Project Configuration

Fleet controllers are also a convenient registry for project configuration import. Import is separate from Fleet sync: it copies selected `metactl.yaml` fields into the current project, while Fleet sync previews or applies generated runtime surfaces across linked projects.

Discover importable projects:

```bash
metactl project import list
metactl project import list --limit 10
metactl project import list --search-root /path/to/repos
```

The human list uses a compact table and shows the first 20 ready projects by default. Use
`--limit 0` to show every row or `--json` when automation needs the complete project list
without display truncation.

Show the importable fields and aliases:

```bash
metactl project import fields
metactl project import fields --json
```

Preview and apply an import by project id, folder name, or direct path:

```bash
metactl project import plan app
metactl project import apply app --yes
metactl setup --import-from /path/to/repos/app --yes
```

By default, import copies `role`, `policy`, `packs`, `targets`, `extends_profile`, `defaults`, and the agent artifact policy metadata. It does not copy `sources`, `linked_projects`, locks, generated files, logs, reports, or arbitrary local state. Use source flags only when the destination should intentionally reference the same libraries. Public and private source records are selected independently:

```bash
metactl project import apply app --include-public-sources --yes
metactl project import apply app --include-private-sources --yes
metactl project import apply app --include-public-sources --include-private-sources --yes
```

When importing into an existing project, choose the write behavior explicitly:

```bash
metactl project import apply app --merge --yes
metactl project import apply app --replace --yes
```

The global file stores only the pointer:

```yaml
fleet:
  default_controller: personal
  controllers:
    personal:
      path: /path/to/private-metactl-library/fleet/personal
```

## Resolution Order

Fleet commands resolve the controller in this order:

1. `--project /path/to/controller`
2. `METACTL_FLEET_CONTROLLER=/path/to/controller`
3. current directory, only when its effective config has `linked_projects`
4. machine-local default controller in `~/.config/metactl/config.yaml`

If no controller is found, metactl exits without mutation and prints the setup command to run.

## Preview And Apply

Preview is the default and does not write linked project files:

```bash
metactl fleet sync --preview
```

> **Expected output**
>
> ```text
> Project: /path/to/controller
> Fleet controller: personal
> Controller source: user_default
> Controller path: /path/to/controller
> Fleet sync preview:
>   metactl  /path/to/repos/metactl  ready
>   app      /path/to/repos/app      ready
> ```

Apply requires explicit automation gates:

```bash
metactl --yes --no-input fleet sync --apply
# Fleet sync apply:
#   metactl  /path/to/repos/metactl  applied
#   app      /path/to/repos/app      applied
```

Fleet apply refuses dirty Git worktrees by default. Use `--allow-dirty` only after review.

Fleet apply updates repo-local generated surfaces in linked projects, including `.codex/skills/...` for Codex CLI targets. It does not install those skills into the user-global Personal picker source under `~/.codex/skills`.

After a Fleet apply, inspect skill scope explicitly:

```bash
metactl fleet status
metactl skills list --scope repo
metactl skills list --scope user
```

Install an operator-facing repo skill globally only when that is intended:

```bash
metactl skills add <repo-skill-path> --scope user
```

## Background Recommendations

Fleet background jobs should refresh recommendation reports, not apply adapter changes. Use the first-class scheduler command:

```bash
metactl --project /path/to/fleet-controller background plan --scope fleet
metactl --project /path/to/fleet-controller background install --scope fleet --yes
metactl --project /path/to/fleet-controller background status --scope fleet
```

On macOS this installs a LaunchAgent. On Linux it installs a systemd user
timer. On Windows it installs a Scheduled Task. The scheduled job writes a
report-only run log, skips disabled Fleet entries, and exits nonzero on any
failed project so monitoring can see drift. Do not put `fleet sync --apply` in
the scheduled job unless a separate operator policy explicitly allows
background mutation.

## Machine Output

`--json` includes the resolved controller:

```json
{
  "command": "fleet",
  "action": "sync",
  "controller": {
    "id": "personal",
    "source": "user_default",
    "path": "/path/to/private-metactl-library/fleet/personal",
    "config_path": "/path/to/private-metactl-library/fleet/personal/metactl.yaml",
    "registry_digest": "..."
  },
  "preview": true,
  "projects": []
}
```

Automation should key on stable project IDs and controller metadata, not human text.
