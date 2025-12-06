# Refactoring TODO

## Priority 3: Connection Split (IN PROGRESS)
- [x] Extract error_handling.rs
- [x] Extract batch_state.rs
- [ ] Create connection/mod.rs
- [ ] Extract handshake.rs (~200 lines)
- [ ] Extract main_loop.rs (~300 lines)
- [ ] Update connection.rs to use submodules

## Priority 4: Handlers Module Split
- [ ] Extract registry.rs (~150 lines)
- [ ] Extract middleware.rs (~100 lines)
- [ ] Extract context.rs (~100 lines)
- [ ] Update handlers/mod.rs to re-export

## Priority 5+: Code Duplication
- [ ] Ban query generic implementation
- [ ] Error reply helper macros
- [ ] Message validation extraction
- [ ] Service command base traits

## Testing & Validation
- [ ] Run cargo test --workspace
- [ ] Run irctest compliance suite
- [ ] Update REFACTORING_PLAN.md status
