# Implementation Plan: Once and OnceCell

> **Phase:** 2 - PLAN  
> **Created:** 2024-12-06  
> **Estimated Duration:** 2-3 hours  
> **Strategy:** Re-export from parking_lot with comprehensive tests

---

## Component Breakdown

### Component 1: Module Structure
**File:** `src/once.rs`  
**Complexity:** Low  
**LOC Estimate:** ~150 lines (mostly tests and docs)

**Purpose:** Create the once module with re-exports and documentation

**Sub-components:**
- Module-level documentation
- Re-export Once from parking_lot
- Re-export OnceCell from parking_lot
- Test module structure

---

### Component 2: Once Tests
**Section:** `#[cfg(test)] mod tests` in `src/once.rs`  
**Complexity:** Medium  
**LOC Estimate:** ~60 lines

**Test Coverage:**
1. Basic single-threaded usage
2. Concurrent initialization
3. State checking (is_completed)
4. Panic during initialization (if supported)

---

### Component 3: OnceCell Tests
**Section:** `#[cfg(test)] mod tests` in `src/once.rs`  
**Complexity:** Medium  
**LOC Estimate:** ~90 lines

**Test Coverage:**
1. Create empty cell
2. Lazy initialization with get_or_init
3. Direct set operation
4. Concurrent access patterns
5. Mutation operations (take)
6. Consumption (into_inner)

---

### Component 4: Library Integration
**File:** `src/lib.rs`  
**Complexity:** Low  
**LOC Estimate:** ~10 lines

**Changes:**
- Add `pub mod once;` declaration
- Re-export Once and OnceCell at crate root
- Update crate-level documentation

---

### Component 5: Documentation Updates
**Files:** Multiple doc files  
**Complexity:** Low  
**LOC Estimate:** ~30 lines

**Updates:**
- `docs/CHANGELOG.md` - Add entry for Once/OnceCell
- `docs/TODO.md` - Mark tasks complete
- README.md - Mention new features (if needed)

---

## Dependency Graph

```
                    ┌─────────────────────┐
                    │ parking_lot = 0.12  │
                    │ (already present)   │
                    └──────────┬──────────┘
                               │
                    ┌──────────▼──────────┐
                    │  Component 1:       │
                    │  Module Structure   │
                    └──────────┬──────────┘
                               │
                 ┌─────────────┼─────────────┐
                 │                           │
      ┌──────────▼──────────┐    ┌──────────▼──────────┐
      │  Component 2:       │    │  Component 3:       │
      │  Once Tests         │    │  OnceCell Tests     │
      └──────────┬──────────┘    └──────────┬──────────┘
                 │                           │
                 └─────────────┬─────────────┘
                               │
                    ┌──────────▼──────────┐
                    │  Component 4:       │
                    │  Library Integration│
                    └──────────┬──────────┘
                               │
                    ┌──────────▼──────────┐
                    │  Component 5:       │
                    │  Documentation      │
                    └─────────────────────┘
```

**Critical Path:** Component 1 → Component 4  
**Parallel Work:** Components 2 and 3 can be written in any order

---

## Task Breakdown Preview

This will decompose into approximately **7-8 atomic tasks**:

1. **T-ONCE-001**: Create src/once.rs with module documentation
2. **T-ONCE-002**: Implement Once tests (4 tests)
3. **T-ONCE-003**: Implement OnceCell tests (6 tests)
4. **T-ONCE-004**: Integrate into src/lib.rs
5. **T-ONCE-005**: Add doc examples to module
6. **T-ONCE-006**: Update CHANGELOG.md
7. **T-ONCE-007**: Update TODO.md

---

## Implementation Sequence

### Phase A: Foundation (30 min)
```
T-ONCE-001: Module structure + re-exports
T-ONCE-004: Library integration
```
**Checkpoint:** `cargo check` passes

### Phase B: Once Implementation (30 min)
```
T-ONCE-002: Once tests
```
**Checkpoint:** `cargo test once::` passes

### Phase C: OnceCell Implementation (45 min)
```
T-ONCE-003: OnceCell tests
```
**Checkpoint:** `cargo test once::` all pass

### Phase D: Documentation (30 min)
```
T-ONCE-005: Doc examples
T-ONCE-006: CHANGELOG update
T-ONCE-007: TODO update
```
**Checkpoint:** All quality gates pass

---

## Test Strategy

### Unit Tests (Minimum 10)

**Once Tests (4 minimum):**
1. `test_once_new` - Create and verify initial state
2. `test_once_call_once` - Single-threaded initialization
3. `test_once_concurrent` - Multi-threaded initialization
4. `test_once_is_completed` - State checking

**OnceCell Tests (6 minimum):**
1. `test_oncecell_new` - Create empty cell
2. `test_oncecell_get_or_init` - Lazy initialization
3. `test_oncecell_set` - Direct initialization
4. `test_oncecell_concurrent` - Multi-threaded access
5. `test_oncecell_take` - Mutable operations
6. `test_oncecell_into_inner` - Consumption

### Doc Tests (Minimum 2)
- Module-level example for Once
- Module-level example for OnceCell

---

## Risk Assessment

### Technical Risks

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| parking_lot API mismatch | Low | High | Pin to version 0.12 |
| Concurrent test flakiness | Medium | Medium | Use deterministic sync patterns |
| Missing parking_lot features | Low | Medium | Verify API completeness first |
| Doc test compilation | Low | Low | Test examples as we write |

### Process Risks

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Scope creep | Low | Medium | Stick to spec, no extras |
| Over-testing | Medium | Low | 10 tests minimum, 15 maximum |
| Under-documentation | Low | High | Doc every public item |

---

## Quality Gates (Repeated for Emphasis)

Every checkpoint MUST pass all gates:

1. ✅ `cargo check` - No compilation errors
2. ✅ `cargo clippy -- -D warnings` - Zero warnings
3. ✅ `cargo test` - All tests pass
4. ✅ `cargo doc --no-deps` - No doc warnings
5. ✅ `cargo fmt --check` - Code formatted

---

## Success Criteria

| Criterion | Definition | Verification Method |
|-----------|------------|---------------------|
| Functionality | Once and OnceCell work correctly | Unit tests pass |
| Concurrency | Thread-safe under concurrent access | Multi-threaded tests |
| Documentation | All public items documented | cargo doc passes |
| Code Quality | No warnings, formatted | clippy + fmt pass |
| Integration | Works with existing primitives | Integration tests |

---

## Estimated Timeline

```
Foundation   (T-ONCE-001, T-ONCE-004)  →  30 min
Once Tests   (T-ONCE-002)               →  30 min
OnceCell     (T-ONCE-003)               →  45 min
Docs         (T-ONCE-005/006/007)       →  30 min
Buffer       (debugging, fixes)         →  30 min
─────────────────────────────────────────────────
Total                                     2h 45min
```

With potential debugging and iteration: **3 hours maximum**

---

## Key Decisions

### KD-001: Re-export Strategy
**Decision:** Re-export parking_lot's Once and OnceCell directly  
**Alternative:** Implement from scratch  
**Chosen because:** 
- Faster implementation
- Battle-tested code
- Consistent with Mutex/RwLock approach
- Lower risk of bugs

### KD-002: Test Coverage
**Decision:** Focus on basic functionality + concurrency  
**Alternative:** Exhaustive edge case testing  
**Chosen because:**
- parking_lot already tested extensively
- We're testing our integration, not parking_lot itself
- 10-15 tests sufficient for confidence

### KD-003: Documentation Depth
**Decision:** Module-level + inline examples  
**Alternative:** Separate examples/ directory  
**Chosen because:**
- API is simple and well-known pattern
- Inline examples are more discoverable
- Can add examples/ later if needed

---

## Next Steps

1. ✅ Plan complete
2. ⏳ Create atomic tasks (Phase 3: TASKS)
3. ⏳ Begin execution (Phase 4: EXECUTE)
4. ⏳ Verify completion (Phase 5: REFLECT)

---

*Plan complete. Ready for task breakdown.*
