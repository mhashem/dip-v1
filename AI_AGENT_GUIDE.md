# AI Agent Guide

This document serves as a context provider and guide for AI agents contributing to this project.

## Project Context
We are building `dip-db`, an embedded, educational database engine in Rust. The architecture follows a traditional layered approach (Disk -> Buffer -> Access -> Execution).

## Coding Standards (Rust)
1.  **Safety:** Avoid `unsafe` unless absolutely necessary (e.g., memory mapping, FFI).
2.  **Error Handling:** Use `Result<T, E>` and the `?` operator. Define custom error types where appropriate.
3.  **Documentation:** Document public structs and methods using `///` comments.
4.  **Testing:** Every module **MUST** have unit tests (`#[test]`) verifying its core functionality.
5.  **Formatting:** Follow `rustfmt` standards.

## Architecture Decisions
-   **Page Size:** 4KB (4096 bytes). This aligns with standard OS page sizes and SSD block sizes.
-   **Page ID:** A unique identifier for a page (usually `u32` or `u64`).
-   **Endianness:** Little-endian is the default for serialization unless specified otherwise.

## Current State
-   **Phase:** Phase 1 (Storage Engine).
-   **Active Module:** `DiskManager` (src/storage/disk_manager.rs).

## Next Steps
After `DiskManager` is verified:
1.  Implement `Replacer` (LRU/Clock) for cache eviction.
2.  Implement `BufferPoolManager`.
