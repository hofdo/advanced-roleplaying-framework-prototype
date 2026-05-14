# Delta Extension Checklist

1. Add the domain type or enum variant in `src/state.rs`.
2. Add serde round-trip coverage in `tests/serde_roundtrip_tests.rs`.
3. Add validation rules in `../engine/src/validation.rs`.
4. Add reducer behavior in `../engine/src/reducer.rs`.
5. Add projection or changed-entity behavior in `../engine/src/projection.rs`.
6. Add API memory-flow coverage when response shape or visible projection changes.
7. Add Postgres coverage when persistence or raw export behavior changes.
8. Update the prompt output contract in `../engine/src/prompt/mod.rs`.
9. Update scenario samples or templates only when the authoring format changes.
