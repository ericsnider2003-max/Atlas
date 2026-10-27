# Private reasoning provider

Atlas already reaches a reasoning model through `LlmConfig` and the `ExternalTool`
contract in `src/brain.rs`. A private model therefore does not require a new
model implementation in the public binary. It requires a user-local `llm:`
configuration whose command/API is private and whose output is validated before
an action is accepted.

The private reasoning model must return an Atlas action object, not shell code:

```json
{"action":"create_reminder","arguments":{"text":"..."},"requires_confirmation":false}
```

Deterministic command parsing remains ahead of this provider. If the private
provider is missing, times out, emits invalid JSON, or returns an unknown action,
Atlas keeps the existing fallback behavior.
