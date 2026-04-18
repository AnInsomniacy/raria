# Logging Contract

Structured logging is a contract, not a best-effort side effect.

The current logging contract covers:

- daemon lifecycle transitions
- restore and shutdown events
- RPC mutation and control-plane entry points
- WebSocket notification emission
- mirror and source failure events
- BT completion lifecycle events

Non-goals:

- exhaustive tracing of every internal branch
- claiming coverage for paths that are not yet backed by tests or rerun evidence
