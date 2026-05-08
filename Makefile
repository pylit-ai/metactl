PYTHON ?= python3
CARGO ?= cargo
VALIDATE_VENV ?= tmp/validate-contracts/.venv
VALIDATE_PYTHON := $(VALIDATE_VENV)/bin/python
VALIDATE_STAMP := $(VALIDATE_VENV)/.installed
FIXTURE_REQUEST ?= fixtures/golden/greenfield-claude-code/jsonrpc/search.request.json

MCP_CLIENT ?= cursor
MCP_SCOPE ?= project
MCP_LIBRARY_ROOT ?= $(CURDIR)/library/starter
MCP_PROJECT_ROOT ?= $(CURDIR)

.PHONY: validate-contracts metactl-validate-contracts metactl-test metactl-check metactl-install metactld-install metactl-mcp-install metactl-mcp-smoke metactl-search-eval metactl-skill-eval run-metactld smoke-stdio smoke-cli smoke-dogfood verify-v1-charter verify-public-boundary verify-docs-links verify-docs-commands verify-mcp-adversarial verify-v1-release-gate verify-v1-lightweight-control-plane verify
validate-contracts: $(VALIDATE_STAMP)
	$(VALIDATE_PYTHON) scripts/validate_contracts.py --include-starter-library --include-targets --include-knowledge-fixtures --library-stack-fixtures

metactl-validate-contracts: validate-contracts

metactl-test:
	$(CARGO) test -p metactl

metactl-check:
	$(CARGO) check -p metactl -p metactld

metactl-search-eval: $(VALIDATE_STAMP)
	$(CARGO) build -p metactl
	$(PYTHON) scripts/evaluate_search.py --metactl-bin ./target/debug/metactl --output tmp/starter-search-eval.json
	$(VALIDATE_PYTHON) -c 'import json, pathlib, sys; sys.path.insert(0, str(pathlib.Path("scripts").resolve())); import validate_contracts; root = pathlib.Path(".").resolve(); registry = validate_contracts.schema_registry(); data = json.loads((root / "tmp/starter-search-eval.json").read_text()); validate_contracts.validate_instance(data, root / "contracts/schemas/metactl/search_eval_artifact.schema.json", registry); print("validated: tmp/starter-search-eval.json")'

metactl-skill-eval: $(VALIDATE_STAMP)
	$(CARGO) build -p metactl
	$(PYTHON) scripts/evaluate_skills.py --metactl-bin ./target/debug/metactl --output tmp/starter-skill-eval.json
	$(VALIDATE_PYTHON) -c 'import json, pathlib, sys; sys.path.insert(0, str(pathlib.Path("scripts").resolve())); import validate_contracts; root = pathlib.Path(".").resolve(); registry = validate_contracts.schema_registry(); data = json.loads((root / "tmp/starter-skill-eval.json").read_text()); validate_contracts.validate_instance(data, root / "contracts/schemas/metactl/skill_eval_artifact.schema.json", registry); print("validated: tmp/starter-skill-eval.json")'

metactl-install:
	$(CARGO) install --path crates/metactl --locked

metactld-install:
	$(CARGO) install --path crates/metactld --locked

metactl-mcp-install: metactld-install
	$(PYTHON) scripts/install_metactl_mcp.py $(MCP_CLIENT) --scope $(MCP_SCOPE) --project-root "$(MCP_PROJECT_ROOT)" --library-root "$(MCP_LIBRARY_ROOT)"

metactl-mcp-smoke:
	$(PYTHON) scripts/smoke_metactl_mcp.py --library-root "$(MCP_LIBRARY_ROOT)"

run-metactld:
	$(CARGO) run -p metactld -- --once $(FIXTURE_REQUEST)

smoke-stdio:
	bash scripts/smoke_stdio.sh

smoke-cli:
	bash scripts/smoke_cli.sh

smoke-dogfood:
	bash scripts/smoke_dogfood.sh

verify-v1-charter:
	$(PYTHON) scripts/verify_v1_charter.py

verify-public-boundary:
	bash scripts/check_public_boundary.sh
	$(PYTHON) scripts/verify_public_boundary.py

verify-docs-links:
	$(PYTHON) scripts/verify_docs_links.py

verify-docs-commands:
	$(CARGO) build -p metactl -p metactld
	$(PYTHON) scripts/verify_docs_commands.py

verify-mcp-adversarial:
	$(CARGO) build -p metactld
	$(PYTHON) scripts/verify_mcp_adversarial.py

verify-v1-release-gate: $(VALIDATE_STAMP)
	$(VALIDATE_PYTHON) scripts/verify_v1_release_gate.py

verify-v1-lightweight-control-plane: $(VALIDATE_STAMP)
	$(VALIDATE_PYTHON) scripts/verify_v1_lightweight_control_plane.py --report tmp/v1-lightweight-control-plane-report.json

verify: verify-v1-charter verify-public-boundary verify-docs-links verify-docs-commands verify-mcp-adversarial verify-v1-release-gate metactl-validate-contracts metactl-test metactl-check smoke-stdio smoke-cli smoke-dogfood

$(VALIDATE_STAMP): requirements-dev.txt
	$(PYTHON) -m venv $(VALIDATE_VENV)
	$(VALIDATE_PYTHON) -m pip install -r requirements-dev.txt
	touch $(VALIDATE_STAMP)
