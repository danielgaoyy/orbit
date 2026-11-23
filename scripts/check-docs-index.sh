#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

ERRORS=0
fail() {
  echo "[check-docs-index] ERROR: $*" >&2
  ERRORS=$((ERRORS + 1))
}

# 1. Root AGENTS.md exists and respects slim size/line budget
if [[ ! -f "AGENTS.md" ]]; then
  fail "Root AGENTS.md is missing."
else
  AGENTS_LINES=$(wc -l < "AGENTS.md" | tr -d ' ')
  AGENTS_BYTES=$(wc -c < "AGENTS.md" | tr -d ' ')
  if [[ "$AGENTS_LINES" -gt 140 ]]; then
    fail "Root AGENTS.md has ${AGENTS_LINES} lines (limit: <= 140 lines). Move details into docs/ or scoped AGENTS.md files."
  fi
  if [[ "$AGENTS_BYTES" -gt 8192 ]]; then
    fail "Root AGENTS.md is ${AGENTS_BYTES} bytes (limit: <= 8192 bytes). Move details into docs/ or scoped AGENTS.md files."
  fi
  if ! grep -Fq "docs/INDEX.md" "AGENTS.md"; then
    fail "Root AGENTS.md must reference docs/INDEX.md."
  fi
  if ! grep -Fq ".agents/skills/interface-design/SKILL.md" "AGENTS.md"; then
    fail "Root AGENTS.md must reference .agents/skills/interface-design/SKILL.md."
  fi
fi

# 2. apps/AGENTS.md exists and mandates .agents/skills/interface-design/SKILL.md
if [[ ! -f "apps/AGENTS.md" ]]; then
  fail "Scoped apps/AGENTS.md is missing."
elif ! grep -Fq ".agents/skills/interface-design/SKILL.md" "apps/AGENTS.md"; then
  fail "apps/AGENTS.md must reference .agents/skills/interface-design/SKILL.md."
fi

# 3. Verify every doc path referenced in docs/INDEX.md exists on disk
if [[ ! -f "docs/INDEX.md" ]]; then
  fail "Master index docs/INDEX.md is missing."
else
  REFERENCED_PATHS=$(
    {
      grep -Eo '^\| `[^` ]+\.md`' "docs/INDEX.md" | sed -E 's/^\| `([^` ]+\.md)`/\1/' || true
      grep -Eo '\([^) ]+\.md\)' "docs/INDEX.md" | tr -d '()' || true
    } \
      | grep -v -E '^(https?:)?//' \
      | grep -v '\*' \
      | sort -u || true
  )

  while IFS= read -r ref; do
    [[ -z "$ref" ]] && continue
    # Normalize ./ prefix or docs/../ prefixes if present
    clean="${ref#./}"
    if [[ "$clean" == ../* ]]; then
      clean="${clean#../}"
    fi
    # Optional generated design-system memory file may not exist yet
    if [[ "$clean" == ".interface-design/system.md" ]]; then
      continue
    fi
    if [[ ! -f "$clean" ]]; then
      fail "docs/INDEX.md references missing markdown file: $ref (resolved: $clean)"
    fi
  done <<< "$REFERENCED_PATHS"

  # 4. Verify all governed doc files in the repo are registered in docs/INDEX.md
  DISCOVERED_DOCS=$(
    {
      find . -maxdepth 1 -type f -name '*.md'
      [[ -d docs ]] && find docs -type f -name '*.md'
      [[ -d deploy ]] && find deploy -maxdepth 1 -type f -name '*.md'
      [[ -d .agents/skills ]] && find .agents/skills -mindepth 2 -maxdepth 2 -type f -name 'SKILL.md'
      find . -mindepth 2 -type f -name 'AGENTS.md' \
        -not -path '*/node_modules/*' \
        -not -path '*/target/*' \
        -not -path '*/.git/*'
    } | sed 's|^\./||' | sort -u
  )

  DOC_COUNT=0
  while IFS= read -r doc; do
    [[ -z "$doc" ]] && continue
    [[ "$doc" == ".github/PULL_REQUEST_TEMPLATE.md" ]] && continue
    DOC_COUNT=$((DOC_COUNT + 1))
    if ! grep -Fq "\`$doc\`" "docs/INDEX.md" && ! grep -Fq "($doc)" "docs/INDEX.md"; then
      fail "Unregistered doc file not found in docs/INDEX.md: $doc"
    fi
  done <<< "$DISCOVERED_DOCS"
fi

if [[ "$ERRORS" -gt 0 ]]; then
  echo "[check-docs-index] FAILED with ${ERRORS} error(s)." >&2
  exit 1
fi

echo "[check-docs-index] PASS: AGENTS.md (${AGENTS_LINES} lines, ${AGENTS_BYTES} bytes), ${DOC_COUNT} governed docs verified against docs/INDEX.md."
