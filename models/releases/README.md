# Released Model Manifests (`models/releases/`)

`models/releases/` holds reviewed, JSON-only public manifests for inference models explicitly selected for release from the private `play-jade/cinnabar` research laboratory.

## Boundary Rules

1. **No binary weights in Git**: Large neural-network checkpoints and NNUE weights live in artifact storage and are downloaded on demand with progress, SHA-256 verification, and size ceilings. Accepting a game invitation never triggers a model download.
2. **Inference state only**: Released weights contain forward-pass parameters only—never optimizer state, replay buffers, credentials, or internal storage paths.
3. **Track honesty**: Each manifest declares its research track (`rules-only-self-play`, `corpus-informed`, or `teacher-assisted`), compatible `rules_profile` (`program-2023.11-v1`), action schema (`from_square * 90 + to_square`, `8,100` labels), observation schema version, runtime backends, and measured evaluation pool conditions. Checkpoints never receive invented Elo ratings.
