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

```text
mediamaster-rag (0.1.0)
└── mediamaster-core (0.3.0, +serde-sync feature)
```

## Quick start

```rust
use mediamaster_rag::MediamasterRagFacade;

let mut rag = MediamasterRagFacade::new();
rag.insert_codec_profile("h264_1080p", 1080, 1920, 1080, "H264", "high");
rag.insert_scene_record("room_101", vec![25.1, 45.0], 120);
```

## Publishing

Published to crates.io: [`mediamaster-rag`](https://crates.io/crates/mediamaster-rag) v0.1.0.
