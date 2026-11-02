# mediamaster-rag

Acoustic profiles, codec matrix & sensor-context knowledge store for `mediamaster-rs`.

## Overview

`mediamaster-rag` is the RAG (retrieval-augmented generation) knowledge layer for the
mediamaster-rs workspace. It provides:

- **Codec knowledge store** (`CodecKnowledgeStore`) — indexed codec capability
  fingerprints (`CodecCorpusEntry`) for hardware-aware codec recommendation.
- **Scene memory store** (`SceneMemoryStore`) — environmental scene records
  (`SceneRecord`) with sensor vectors (`SensorVector`) and temporal metadata for
  context-aware acoustic tuning.
- **Playbook store** (`PlaybookStore`) — acoustic paging playbooks
  (`PagingDuckingPlaybook`) with ducking profiles and trigger patterns for
  automated room-level audio ducking.

## Crate graph

```
mediamaster-rag (0.1.0)
└── mediamaster-core (0.3.0, +serde-sync feature)
```

## Quick start

```rust
use mediamaster_rag::MediamasterRagFacade;

// Construct the facade and query the codec corpus
let rag = MediamasterRagFacade::new();
```

## License

Apache-2.0 (see `LICENSE`).
