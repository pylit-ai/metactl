#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

check_file() {
  local rel="$1"
  local max_lines="$2"
  local max_functions="$3"
  local file="$ROOT/$rel"

  if [[ ! -f "$file" ]]; then
    echo "missing architecture file: $rel" >&2
    return 1
  fi

  local lines
  local functions
  lines="$(wc -l < "$file" | tr -d ' ')"
  functions="$(
    awk '
      /^[[:space:]]*(pub(\([^)]*\))?[[:space:]]+)?(async[[:space:]]+)?fn[[:space:]]+/ {
        count += 1
      }
      END { print count + 0 }
    ' "$file"
  )"

  printf '%s lines=%s/%s functions=%s/%s\n' "$rel" "$lines" "$max_lines" "$functions" "$max_functions"

  if (( lines > max_lines )); then
    echo "architecture metric failed: $rel has $lines lines, max $max_lines" >&2
    return 1
  fi

  if (( functions > max_functions )); then
    echo "architecture metric failed: $rel has $functions functions, max $max_functions" >&2
    return 1
  fi
}

check_file "crates/metactl/src/main.rs" 11750 300
check_file "crates/metactl/src/library_registry.rs" 3375 121
