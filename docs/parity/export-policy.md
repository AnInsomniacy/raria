# Export Policy

The repository treats discoverable RPC and WebSocket symbols as public contract.

Truth source order:

1. live module behavior
2. generated manifests in `.omx/parity/generated`
3. declared policy in `.omx/parity/exported-surface.yaml`
4. README and release prose

Export classes:

- `aria2_parity_surface`
  - requires a legacy anchor
  - starts as `provisional`
  - becomes `ready` only when the corresponding parity entry is `Matched` or approved `AcceptableDelta`
- `raria_extension_surface`
  - explicitly not counted as parity completion
  - may become `ready` only when documentation is honest about extension status

Current default decision:

- `aria2.onSourceFailed` is a `raria_extension_surface`

That symbol may remain exposed, but it must not be used as evidence that the legacy aria2 notification set is complete.
