# Scenario Authoring CLI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add scenario validation, inspection, and sample/template workflows to the `rp scenario` CLI so authors can check scenario JSON before creating sessions.

**Architecture:** Keep parsing and domain validation in `scenario_io.rs`, keep command wiring in `commands/scenario.rs`, and keep sample/template discovery in `samples.rs` and build-script generated registries. The CLI should validate local files without requiring API storage.

**Tech Stack:** Rust, Cargo workspace, Axum, SQLx/Postgres, Clap CLI, serde, tokio tests

---

## Current State

- `crates/cli/src/commands/scenario.rs` supports `create`, `list`, `get`, and `delete`.
- `crates/cli/src/scenario_io.rs` parses JSON and calls `domain::validate_scenario`.
- `crates/cli/src/samples.rs` loads built-in sample scenarios and validates them.
- `crates/cli/scenarios/templates/scenario.template.json` is validated by `samples.rs` tests.
- `crates/cli/tests/cli_smoke.rs` covers end-to-end CLI scenario/session/turn/world cycles.
- Domain validation currently catches duplicate IDs, unknown NPC initial locations, bad clocks, and out-of-range faction standings.

## Target Behavior

- `rp scenario validate --file PATH` validates a scenario file and prints a concise success summary or a clear validation error.
- `rp scenario inspect --file PATH` prints an author-friendly summary of locations, NPCs, factions, quests, secrets, clocks, and opening state assumptions.
- `rp scenario samples` lists built-in sample names.
- `rp scenario template` prints the bundled template JSON.
- Commands work in memory mode without creating a persisted scenario.

## File Structure

- Modify: `crates/cli/src/commands/scenario.rs`
  - Add `Validate`, `Inspect`, `Samples`, and `Template` subcommands.
- Modify: `crates/cli/src/scenario_io.rs`
  - Add a summary builder that returns structured counts and warnings.
- Modify: `crates/cli/src/samples.rs`
  - Expose template text if not already available through `include_str!`.
- Modify: `crates/cli/tests/cli_smoke.rs`
  - Add command-level smoke tests for validation, inspection, samples, and template output.
- Optional modify: `crates/domain/src/validation.rs`
  - Add focused validation errors only when the CLI tests reveal missing domain checks.

## Tasks

### Task 1: Add Validate Command

**Files:**
- Modify: `crates/cli/src/commands/scenario.rs`
- Modify: `crates/cli/tests/cli_smoke.rs`

- [ ] **Step 1: Write failing CLI test**

Add a smoke test that writes a valid scenario JSON to a temp file, runs:

```bash
cargo run -p cli -- scenario validate --file <PATH>
```

and asserts stdout contains:

```text
valid scenario:
title:
locations:
npcs:
```

Also add an invalid duplicate-location file and assert the command exits non-zero with stderr containing `duplicate location id`.

- [ ] **Step 2: Run expected failing command**

Run: `cargo test -p cli --test cli_smoke scenario_validate`

Expected: failure because the `validate` subcommand does not exist.

- [ ] **Step 3: Implement command enum variant**

Add to `Cmd`:

```rust
Validate {
    #[arg(long)]
    file: String,
},
```

In `run`, call `read_scenario_file(&file)?` and print:

```text
valid scenario:
title: <title>
locations: <count>
npcs: <count>
factions: <count>
quests: <count>
secrets: <count>
clocks: <count>
```

- [ ] **Step 4: Run passing test**

Run: `cargo test -p cli --test cli_smoke scenario_validate`

Expected: the valid case exits successfully and the invalid case fails with the domain validation message.

- [ ] **Step 5: Commit**

```bash
git add crates/cli/src/commands/scenario.rs crates/cli/tests/cli_smoke.rs
git commit -m "feat(cli): validate scenario files"
```

### Task 2: Add Inspect Command

**Files:**
- Modify: `crates/cli/src/scenario_io.rs`
- Modify: `crates/cli/src/commands/scenario.rs`
- Modify: `crates/cli/tests/cli_smoke.rs`

- [ ] **Step 1: Write failing summary unit test**

Add a test in `scenario_io.rs`:

```rust
#[test]
fn scenario_summary_names_opening_entities() {
    let scenario = parse_scenario_json(include_bytes!("../scenarios/templates/scenario.template.json"), "template")
        .expect("template parses");
    let summary = scenario_summary(&scenario);

    assert!(summary.contains("Opening location:"));
    assert!(summary.contains("Opening speaker:"));
    assert!(summary.contains("Clock count:"));
}
```

- [ ] **Step 2: Run expected failing command**

Run: `cargo test -p cli scenario_summary_names_opening_entities`

Expected: fails because `scenario_summary` does not exist.

- [ ] **Step 3: Implement summary builder**

Add:

```rust
pub fn scenario_summary(scenario: &Scenario) -> String
```

Include exact lines for title, tone, opening location (`locations.first()`), opening speaker (`npcs.first()`), counts, hidden NPC count, and secret count. Keep it plain text for terminal readability.

- [ ] **Step 4: Wire CLI command**

Add:

```rust
Inspect {
    #[arg(long)]
    file: String,
},
```

Call `read_scenario_file`, then print `scenario_summary(&scenario)`.

- [ ] **Step 5: Run tests**

Run: `cargo test -p cli scenario_summary_names_opening_entities`

Run: `cargo test -p cli --test cli_smoke scenario_inspect`

Expected: both pass.

- [ ] **Step 6: Commit**

```bash
git add crates/cli/src/scenario_io.rs crates/cli/src/commands/scenario.rs crates/cli/tests/cli_smoke.rs
git commit -m "feat(cli): inspect scenario files"
```

### Task 3: Add Samples And Template Commands

**Files:**
- Modify: `crates/cli/src/samples.rs`
- Modify: `crates/cli/src/commands/scenario.rs`
- Modify: `crates/cli/tests/cli_smoke.rs`

- [ ] **Step 1: Write failing tests**

Add smoke tests:

```rust
#[test]
fn scenario_samples_lists_builtin_names() {
    let output = run_cli(["scenario", "samples"]);
    assert!(output.status.success());
    assert!(stdout(&output).contains("bride-of-the-iron-archduke"));
}

#[test]
fn scenario_template_prints_valid_json() {
    let output = run_cli(["scenario", "template"]);
    assert!(output.status.success());
    let scenario: domain::Scenario = serde_json::from_str(&stdout(&output)).expect("template json");
    domain::validate_scenario(&scenario).expect("template validates");
}
```

- [ ] **Step 2: Run expected failing command**

Run: `cargo test -p cli --test cli_smoke scenario_samples scenario_template`

Expected: fails because the subcommands are missing.

- [ ] **Step 3: Expose template text**

In `samples.rs`, add:

```rust
pub fn template_json() -> &'static str {
    include_str!("../scenarios/templates/scenario.template.json")
}
```

- [ ] **Step 4: Wire commands**

Add `Samples` and `Template` to `Cmd`. `Samples` prints one sample name per line from `sample_names()`. `Template` prints `template_json()` exactly.

- [ ] **Step 5: Run tests**

Run: `cargo test -p cli --test cli_smoke scenario_samples scenario_template`

Expected: tests pass and the template remains valid JSON.

- [ ] **Step 6: Commit**

```bash
git add crates/cli/src/samples.rs crates/cli/src/commands/scenario.rs crates/cli/tests/cli_smoke.rs
git commit -m "feat(cli): expose scenario samples and template"
```

## Verification

Run:

```bash
cargo test -p cli
cargo run -p cli -- scenario samples
cargo run -p cli -- scenario template
cargo run -p cli -- scenario validate --file crates/cli/scenarios/templates/scenario.template.json
cargo run -p cli -- scenario inspect --file crates/cli/scenarios/samples/bride-of-the-iron-archduke.json
```

## Acceptance Criteria

- Scenario authors can validate a JSON file without creating persistent data.
- Scenario authors can inspect counts and opening assumptions from the terminal.
- Built-in sample names and the authoring template are discoverable through `rp scenario`.
- Invalid scenarios fail with domain validation messages.
- Existing `create`, `list`, `get`, and `delete` commands keep working.

## Risks

- CLI output can become a compatibility surface. Keep summary text stable and simple.
- Domain validation gaps may appear as inspect warnings; only add domain errors when invalid data would break runtime behavior.
- Tests that invoke `cargo run` can be slower than unit tests; prefer existing CLI smoke helper style.

