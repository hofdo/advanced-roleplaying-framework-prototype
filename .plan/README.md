# Roadmap Implementation Plans

This directory contains grouped implementation plans for the current roadmap. Existing flat files directly under `.plan/` are historical planning notes and are intentionally left untouched.

Each plan is written for a fresh agentic worker. The implementation files use checkbox steps, name the files to inspect or edit, include concrete verification commands, and keep acceptance criteria explicit.

## Architecture

1. [Non-Streaming Secrecy Boundary](architecture/01-non-streaming-secrecy-boundary.md)
2. [Docker-Backed CI](architecture/02-docker-backed-ci.md)
3. [Docs Current State](architecture/03-docs-current-state.md)
4. [Engine Module Decomposition](architecture/04-engine-module-decomposition.md)
5. [State Delta Extensibility](architecture/05-state-delta-extensibility.md)

## Features

1. [Scenario Authoring CLI](features/01-scenario-authoring-cli.md)
2. [Campaign Memory](features/02-campaign-memory.md)
3. [Session Timeline Debugger](features/03-session-timeline-debugger.md)
4. [Provider Session UX](features/04-provider-session-ux.md)
5. [Replayable Exports And Fixtures](features/05-replayable-exports-fixtures.md)

## Gameplay

1. [Action Resolution With Stakes](gameplay/01-action-resolution-with-stakes.md)
2. [Player Character State](gameplay/02-player-character-state.md)
3. [Relationships And Faction Pressure](gameplay/03-relationships-faction-pressure.md)
4. [Secrets, Clues, And Discovery](gameplay/04-secrets-clues-discovery.md)
5. [NPC Agency](gameplay/05-npc-agency.md)
6. [Iron Archduke Scenario Mechanics](gameplay/06-iron-archduke-scenario-mechanics.md)

## Recommended Execution Order

Run the architecture plans before gameplay expansion:

1. `architecture/01-non-streaming-secrecy-boundary.md`
2. `architecture/02-docker-backed-ci.md`
3. `architecture/03-docs-current-state.md`
4. `architecture/05-state-delta-extensibility.md`
5. Feature and gameplay plans in dependency order
6. `architecture/04-engine-module-decomposition.md` after behavior is locked by tests

The gameplay plans depend on the same core surfaces: `domain::WorldState`, `domain::WorldStateDelta`, engine validation/reduction/projection, persistence JSON state storage, prompts, and API/CLI tests.

