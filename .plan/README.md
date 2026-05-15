# Roadmap Implementation Plans

This directory is a historical index of the implementation plans that shaped the current codebase. The numbered plans under `architecture/`, `features/`, and `gameplay/` are kept as reference material for the work that already landed in the repository.

The flat files directly under `.plan/` are older planning notes and are intentionally left untouched.

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

## Current State

The architecture, feature, and gameplay plans are already reflected in the repository. Use these files as a map back to the implementation work rather than as a live execution queue.

The gameplay plans share the same core surfaces: `domain::WorldState`, `domain::WorldStateDelta`, engine validation/reduction/projection, persistence JSON state storage, prompts, and API/CLI tests.

If you need to compare the code against the original plan intent, start with the numbered plan closest to the area you are touching and then read the corresponding crate README for the current implementation details.
