#!/usr/bin/env bash
#
# spec_diff.sh -- compare Rust public struct fields to OpenAPI spec schemas.
#
# Usage:
#   tools/audit/spec_diff.sh > AUDIT_DIFF.md
#
# Reads pairs from PAIRS array; for each (rust_file, rust_struct,
# spec_schema), emits a markdown section with field-level differences.
#
# Limitations:
#   - Resolves #[serde(rename = "...")] when the rename is on the line
#     immediately above the field. Multi-line attribute blocks may miss.
#   - Does not validate types, only field name presence.
#   - Spec may have inline schemas (object types defined inline) not
#     pulled by name; those need separate inspection.

set -euo pipefail

SPEC="${SPEC:-${HOME}/claude-api-docs/openapi.json}"
SRC_ROOT="${SRC_ROOT:-src}"

if [[ ! -f "$SPEC" ]]; then
  echo "ERROR: spec not found at $SPEC" >&2
  exit 1
fi

# Extract pub field names from a Rust struct, applying #[serde(rename)] if
# present on the immediately-preceding line. Emits one wire-name per line.
rust_fields() {
  local file="$1" struct="$2"
  awk -v want="$struct" '
    BEGIN { in_struct = 0; pending_rename = "" }
    /pub struct/ {
      # Capture the struct name token
      if (match($0, /pub struct [A-Z][A-Za-z0-9_]*/)) {
        name = substr($0, RSTART+11, RLENGTH-11)
        if (name == want) {
          in_struct = 1
          pending_rename = ""
          next
        }
      }
    }
    in_struct && /^}/ { in_struct = 0; next }
    !in_struct { next }

    # Track #[serde(rename = "...")] on the previous line
    /#\[serde.*rename *= *"[^"]*"/ {
      match($0, /rename *= *"[^"]*"/)
      r = substr($0, RSTART, RLENGTH)
      gsub(/rename *= *"|"/, "", r)
      pending_rename = r
      next
    }

    # Field declaration: `    pub <ident>:` (skip if it is a method def)
    /^[[:space:]]+pub [a-z_][a-z0-9_]*:/ {
      match($0, /pub [a-z_][a-z0-9_]*:/)
      field = substr($0, RSTART+4, RLENGTH-5)
      if (pending_rename != "") {
        print pending_rename
      } else {
        print field
      }
      pending_rename = ""
      next
    }

    # Anything else clears pending rename
    !/^[[:space:]]*$/ && !/^[[:space:]]*\/\// {
      pending_rename = ""
    }
  ' "$file"
}

# Extract property names from an OpenAPI schema. Resolves $ref one level
# (so Foo -> Bar where Foo is `$ref: "#/components/schemas/Bar"`).
spec_fields() {
  local schema="$1"
  jq -r --arg s "$schema" '
    def resolve(name):
      .components.schemas[name] as $sch |
      if $sch.["$ref"] then
        ($sch["$ref"] | sub("#/components/schemas/"; "")) as $r |
        resolve($r)
      elif $sch.allOf then
        # allOf: union of all referenced schemas
        ([$sch.allOf[] |
          if .["$ref"] then
            (.["$ref"] | sub("#/components/schemas/"; "")) as $r |
            resolve($r)
          else
            (.properties // {}) | keys
          end
        ] | flatten | unique)
      else
        ($sch.properties // {}) | keys
      end;
    resolve($s)[]?
  ' "$SPEC"
}

diff_one() {
  local file="$1" struct="$2" schema="$3"
  local rust_list spec_list rust_only spec_only
  rust_list=$(rust_fields "$file" "$struct" | sort -u)
  spec_list=$(spec_fields "$schema" | sort -u)
  if is_tagged_variant "$schema"; then
    spec_list=$(echo "$spec_list" | grep -vx 'type' || true)
  fi

  if [[ -z "$rust_list" ]]; then
    echo "### \`$struct\` (file: \`$file\`) — schema \`$schema\`"
    echo
    echo "**WARN**: no Rust fields extracted (struct not found or empty)"
    echo
    return
  fi
  if [[ -z "$spec_list" ]]; then
    echo "### \`$struct\` — schema \`$schema\`"
    echo
    echo "**WARN**: no spec fields extracted (schema not found or empty)"
    echo
    return
  fi

  rust_only=$(comm -23 <(echo "$rust_list") <(echo "$spec_list") || true)
  spec_only=$(comm -13 <(echo "$rust_list") <(echo "$spec_list") || true)
  shared=$(comm -12 <(echo "$rust_list") <(echo "$spec_list") | wc -l | tr -d ' ')
  rust_count=$(echo "$rust_list" | wc -l | tr -d ' ')
  spec_count=$(echo "$spec_list" | wc -l | tr -d ' ')

  if [[ -z "$rust_only" && -z "$spec_only" ]]; then
    echo "### \`$struct\` — \`$schema\`  ✓ ($shared/$shared)"
    echo
    return
  fi

  echo "### \`$struct\` — \`$schema\`  (rust=$rust_count, spec=$spec_count, shared=$shared)"
  echo
  if [[ -n "$spec_only" ]]; then
    echo "**Missing in Rust** (in spec but not in our struct):"
    echo
    echo "$spec_only" | sed 's/^/- `/' | sed 's/$/`/'
    echo
  fi
  if [[ -n "$rust_only" ]]; then
    echo "**Extra in Rust** (in our struct but not in spec):"
    echo
    echo "$rust_only" | sed 's/^/- `/' | sed 's/$/`/'
    echo
  fi
}

# Schemas where `type` is the enum-tag discriminator (handled by
# serde at the SessionEvent enum level via #[serde(tag = "type")]).
# We strip `type` from these spec field lists to avoid false positives.
TAGGED_VARIANTS=(
  "BetaManagedAgentsUserMessageEvent"
  "BetaManagedAgentsUserCustomToolResultEvent"
  "BetaManagedAgentsUserToolConfirmationEvent"
  "BetaManagedAgentsUserInterruptEvent"
  "BetaManagedAgentsAgentMessageEvent"
  "BetaManagedAgentsAgentToolUseEvent"
  "BetaManagedAgentsAgentToolResultEvent"
  "BetaManagedAgentsAgentCustomToolUseEvent"
  "BetaManagedAgentsAgentMcpToolUseEvent"
  "BetaManagedAgentsAgentMcpToolResultEvent"
  "BetaManagedAgentsAgentThinkingEvent"
  "BetaManagedAgentsAgentThreadContextCompactedEvent"
  "BetaManagedAgentsSpanModelRequestStartEvent"
  "BetaManagedAgentsSpanModelRequestEndEvent"
  "BetaManagedAgentsSessionStatusRescheduledEvent"
  "BetaManagedAgentsSessionStatusRunningEvent"
  "BetaManagedAgentsSessionStatusIdleEvent"
  "BetaManagedAgentsSessionStatusTerminatedEvent"
  "BetaManagedAgentsSessionDeletedEvent"
  "BetaManagedAgentsSessionErrorEvent"
)

is_tagged_variant() {
  local s="$1"
  for v in "${TAGGED_VARIANTS[@]}"; do
    [[ "$s" == "$v" ]] && return 0
  done
  return 1
}

# (rust_file:rust_struct:spec_schema) tuples to compare.
# Mapping uses the canonical response schema discovered via
# /tmp/discover_schemas.sh.
PAIRS=(
  # Managed Agents core
  "src/managed_agents/sessions.rs:Session:BetaManagedAgentsSession"
  "src/managed_agents/sessions.rs:SessionUsage:BetaManagedAgentsSessionUsage"
  "src/managed_agents/agents.rs:Agent:BetaManagedAgentsAgent"
  "src/managed_agents/environments.rs:Environment:BetaEnvironment"
  "src/managed_agents/vaults.rs:Vault:BetaManagedAgentsVault"
  "src/managed_agents/vaults.rs:Credential:BetaManagedAgentsCredential"
  "src/managed_agents/memory_stores.rs:MemoryStore:BetaManagedAgentsGetMemoryStoreResponse"
  "src/managed_agents/memory_stores.rs:Memory:BetaManagedAgentsMemory"
  "src/managed_agents/memory_stores.rs:MemoryVersion:BetaManagedAgentsMemoryVersion"
  "src/managed_agents/resources.rs:FileResource:BetaManagedAgentsFileResource"
  "src/managed_agents/resources.rs:GitHubRepositoryResource:BetaManagedAgentsGitHubRepositoryResource"
  "src/managed_agents/resources.rs:MemoryStoreResource:BetaManagedAgentsMemoryStoreResource"

  # Top-level resources
  "src/messages/response.rs:Message:BetaMessage"
  "src/batches/types.rs:MessageBatch:BetaMessageBatch"
  "src/files/types.rs:FileMetadata:BetaFileMetadataSchema"
  "src/models/mod.rs:ModelInfo:BetaModelInfo"
  "src/skills.rs:Skill:BetaGetSkillResponse"
  "src/skills.rs:SkillVersion:BetaGetSkillVersionResponse"
  "src/user_profiles.rs:UserProfile:BetaUserProfile"

  # Events (type field stripped for tagged variants)
  "src/managed_agents/events.rs:UserMessageEvent:BetaManagedAgentsUserMessageEvent"
  "src/managed_agents/events.rs:UserCustomToolResultEvent:BetaManagedAgentsUserCustomToolResultEvent"
  "src/managed_agents/events.rs:UserToolConfirmationEvent:BetaManagedAgentsUserToolConfirmationEvent"
  "src/managed_agents/events.rs:AgentMessageEvent:BetaManagedAgentsAgentMessageEvent"
  "src/managed_agents/events.rs:AgentToolUseEvent:BetaManagedAgentsAgentToolUseEvent"
  "src/managed_agents/events.rs:AgentToolResultEvent:BetaManagedAgentsAgentToolResultEvent"
  # NOTE: AgentCustomToolUse, AgentMcpToolUse, AgentMcpToolResult use
  # inline enum-variant payloads (not separate structs); compare manually.
  "src/managed_agents/events.rs:SpanModelRequestEndEvent:BetaManagedAgentsSpanModelRequestEndEvent"
  "src/managed_agents/events.rs:SessionErrorEvent:BetaManagedAgentsSessionErrorEvent"
  "src/managed_agents/events.rs:SessionStatusIdleEvent:BetaManagedAgentsSessionStatusIdleEvent"
)

# Header
echo "# Spec-diff report"
echo
echo "Generated by \`tools/audit/spec_diff.sh\` from \`$SPEC\`."
echo
echo "Source of truth: OpenAPI spec field names (with \`\$ref\` and \`allOf\`"
echo "resolution). Rust side resolves \`#[serde(rename = ...)]\` when on the"
echo "line immediately preceding the field."
echo
echo "Legend: \`✓\` = exact match. Otherwise the section lists the diff."
echo
echo "## Summary"
echo

# First pass: count
total=${#PAIRS[@]}
matched=0
mismatched_pairs=()
for pair in "${PAIRS[@]}"; do
  IFS=':' read -r f s schema <<<"$pair"
  if [[ ! -f "$f" ]]; then
    mismatched_pairs+=("$pair (FILE MISSING)")
    continue
  fi
  out=$(diff_one "$f" "$s" "$schema")
  if echo "$out" | head -1 | grep -q "✓"; then
    matched=$((matched + 1))
  else
    mismatched_pairs+=("$pair")
  fi
done

echo "$matched / $total pairs match exactly."
echo
if [[ ${#mismatched_pairs[@]} -gt 0 ]]; then
  echo "Mismatches or warnings:"
  for p in "${mismatched_pairs[@]}"; do
    echo "- \`$p\`"
  done
fi
echo

echo "## Pair-by-pair details"
echo
for pair in "${PAIRS[@]}"; do
  IFS=':' read -r f s schema <<<"$pair"
  if [[ ! -f "$f" ]]; then
    echo "### \`$s\` (file: \`$f\`) — schema \`$schema\`"
    echo
    echo "**WARN**: source file missing"
    echo
    continue
  fi
  diff_one "$f" "$s" "$schema"
done
