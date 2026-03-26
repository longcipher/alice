# alice-core

Core domain types and port traits for the [Alice](https://github.com/longcipher/alice) AI agent.

## Overview

This crate provides the innermost layer of the Alice hexagonal architecture with zero adapter dependencies:

- **Memory domain types** — `MemoryEntry`, `RecallQuery`, `RecallHit`, `HybridWeights`, `MemoryImportance`
- **Memory port trait** — `MemoryStorePort` for pluggable persistence backends
- **Memory service** — recall, persist, and render memory context
- **Hybrid scoring** — BM25 + vector similarity fusion, FTS query sanitization

## Usage

```rust
use alice_core::memory::{
    domain::{HybridWeights, MemoryEntry, MemoryImportance},
    ports::MemoryStorePort,
    service::MemoryService,
};

// Create a memory service with any MemoryStorePort implementation
let service = MemoryService::new(
    store,          // Arc<dyn MemoryStorePort>
    6,              // recall_limit
    HybridWeights::default(),  // bm25=0.3, vector=0.7
    384,            // vector_dimensions
    true,           // enable_vector
)?;

// Recall relevant memories for a turn
let hits = service.recall_for_turn("session-1", "what did we discuss about SQLite?")?;

// Persist a conversation turn
service.persist_turn("session-1", "user message", "assistant response")?;
```

## License

Apache-2.0
