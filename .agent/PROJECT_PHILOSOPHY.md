# slircd-ng Project Philosophy

## Development Approach

**Status**: Pre-release, zero users

**Philosophy**: Direct implementation, no migrations

### Core Principles

1. **Direct Implementation**
   - Make decisions and implement immediately
   - No gradual migrations or backwards compatibility concerns
   - All changes in one pass, test at end

2. **Clean Code**
   - No legacy cruft
   - No deprecated code paths
   - Delete old code, don't comment it out

3. **Efficiency**
   - Minimize token usage
   - Avoid over-engineering (no atomic commit strategies)
   - Single commit per feature when appropriate

4. **Testing Strategy**
   - Implement all changes
   - Test at the end
   - Fix issues as they arise

### What This Means

- ✅ Make all changes in one go
- ✅ Test comprehensively at the end
- ✅ Single commits for cohesive features
- ❌ No migration paths
- ❌ No backwards compatibility
- ❌ No gradual rollouts
