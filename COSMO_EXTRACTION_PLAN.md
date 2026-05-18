# Cosmo/OOPS Shared Library Extraction Plan

## What We Adapted First

OOPS now has a first Cosmo-inspired large-file path for `edge_crossing_edge`.

- `osm_pbf_core::node_store::NodeStoreWriter` / `NodeStoreReader` adapts Cosmo's sparse mmap node-cache pattern.
- `LargeEdgeCrossingReader` performs two PBF passes: first index nodes, then resolve only road ways into geometries.
- `oops-cli --stream-large` bypasses full `OsmData` loading for `edge_crossing_edge`.
- Large GeoJSON output is streamed as a FeatureCollection instead of collecting all features first.

This is intentionally not a generic analyzer rewrite. It proves the large-file interfaces with one spatial-pair analyzer.

## Extraction Candidates

Created a shared crate used by both Cosmo and OOPS: `osm-pbf-core`.

## Ecosystem Positioning

GeoRust and the Rust OSM ecosystem already cover the lower layers well:

- `osmpbf`, `osmpbfreader`, and `osm-io` read OSM PBF data.
- `geo`, `geo-types`, `rstar`, `geozero`, and `geojson` cover geometry, spatial indexing, format conversion, and feature output.
- `osmnodecache` already targets dense planet-scale node-coordinate caching.
- `osm-primitives` covers reusable OSM data model primitives.

Therefore `osm-pbf-core` should not become a new parser, geometry engine, spatial index, or OSM domain model. Its role is the application pipeline layer both OOPS and Cosmo need: bounded PBF passes, node lookup strategies, streaming sinks, bbox/progress helpers, and eventually tile/block orchestration.

Initial modules:

- `node_store`: sparse mmap node cache first; later dense and memory modes.
- `pbf_pass`: reusable BlobReader/ElementReader pass helpers, progress hooks, and bounded writer-channel patterns.
- `sinks`: streaming GeoJSON and GeoJSONL writers with a small feature-row abstraction.
- `bbox`: shared bbox parsing, validation, and geometry intersection helpers.
- `progress`: lightweight counters usable from both CLI tools.

## Keep Project-Specific

Do not extract analyzer logic. OOPS checks need finding severity, confidence, dedupe keys, and OSM QA semantics. Cosmo conversion needs filters, tables, columns, and geometry modes.

Keep these separate:

- OOPS `Analyzer`, `Finding`, and `AnalysisResult`.
- Cosmo YAML filters, DSL, CEL, and table mapping.
- OOPS edge-crossing pair detection and future tiled execution.

## Lessons From This Pass

- Sparse node cache is immediately useful and much safer than retaining all nodes in memory.
- Streaming GeoJSON is low risk and should become the default for large outputs.
- The large OOPS path still keeps all resolved road geometries in memory; the next step is tile spooling.
- Dense node cache needs a presence marker before reuse in OOPS. Cosmo's dense zero default can hide missing nodes as `(0, 0)`.
- Sparse cache assumes sorted PBF node IDs. Keep that explicit and return a clear error.

## Next Refactor

1. Add explicit missing-node handling to dense node-store mode, or wrap `osmnodecache` instead.
2. Extract reusable PBF pass helpers from OOPS and Cosmo once their large-file callers stabilize.
3. Replace `LargeEdgeCrossingReader`'s all-roads vector with tile spool files.
4. Process tiles independently with overlap and dedupe by `(min_way_id, max_way_id, crossing_point)`.
5. Add `--jobs` to OOPS large mode and parallelize tile processing after output coordination is stable.
