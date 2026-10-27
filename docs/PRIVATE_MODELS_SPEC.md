# Private models for Atlas and UGC

This is the boundary between the public Atlas product and Eric's private
models. The public product may contain provider interfaces, generic fallback
models, and documentation. It must not contain Eric's recordings, voice
weights, reasoning adapters, memory, or credentials.

## Artifact manifest

Each private artifact is described by a JSON manifest:

```json
{
  "schema": 1,
  "kind": "voice | reasoning",
  "model_id": "eric-f5-v1 | atlas-reasoning-v1",
  "runtime": "f5_tts | llama_cpp | command",
  "entrypoint": "absolute-or-user-relative-runtime-entrypoint",
  "artifact_dir": "absolute-or-user-relative-private-directory",
  "sha256": "sha256-of-the-versioned-artifact",
  "license": "base-model-license-and-adapter-notice",
  "dataset_version": "dataset-id-used-for-training"
}
```

The loader must reject missing fields, unsupported `kind`/`runtime`, paths
that escape the configured private root, and manifests whose artifact hash does
not match the recorded hash. A manifest is metadata, not permission to load an
arbitrary executable: the runtime must be selected from an allowlisted local
provider.

## Private roots

The default public Atlas tree is never a private root. A user opts in with a
user-local setting, for example:

```text
ATLAS_PRIVATE_MODELS=C:\Users\<user>\Atlas-private
```

The directory contains `voice/manifest.json`, `reasoning/manifest.json`, and
their artifacts. It is outside the repository and outside release packaging.
UGC may point at the same voice manifest, but it must not copy it into the
public UGC or Atlas source tree.

## Runtime behavior

- Missing or invalid private artifacts produce a visible diagnostic and use the
  configured public fallback.
- A private provider is opt-in; it is never enabled by a public default.
- A provider receives text or a structured Atlas task envelope and returns only
  validated output. It cannot issue shell commands directly.
- Voice output must be a mono 24 kHz WAV before playback or UGC mixing.
- Reasoning output must validate against the Atlas action schema before any
  action is considered.
- Personal memory remains local retrieved context. It is not part of the
  reasoning model checkpoint.

## Training boundary

Kaggle is an authorized GPU training environment, not a runtime dependency.
Training exports must include a versioned manifest, artifact hash, dataset
version, license notice, and evaluation report. The laptop performs dataset
preparation, smoke inference, evaluation, and final Atlas/UGC integration.

## Public-package rule

Release verification fails if a public package contains a private root, private
manifest, Eric audio, private checkpoint, personal memory file, credential, or
machine-specific private path. The public fallback must still boot and speak
when no private root exists.
