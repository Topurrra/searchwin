# embedding-runtime — semantic search model (beta)

This directory holds the bundled model for **semantic file search** (the
"Semantic search (beta)" toggle under Search → Advanced indexing). It is
empty in the repo; the model files are added on the **build machine** as the
final activation step. Everything below is offline — no HuggingFace download,
no network at build or runtime, same posture as the Vosk / Tesseract / PDFium
runtimes.

The plumbing (ranking signal, vector store, query + index wiring, the config
toggle, the Settings UI) is already merged and **compiles + runs inert**
without any of this — `embedding::embed` returns `None` until the `semantic`
feature is built AND these files are present, so the app behaves exactly as
before until you complete the steps here.

## Model: all-MiniLM-L6-v2 (Apache-2.0)

Chosen deliberately: **Apache-2.0** (unlike `bge-small`, which is CC-BY-NC and
cannot ship in a paid product), symmetric (no query/passage prefixes), 384-d,
~23 MB int8. Put these four files in THIS directory (filenames must match —
see `src-tauri/src/commands/embedding.rs::load_model`):

```
embedding-runtime/
  model.onnx                 # the int8 ONNX weights (~23 MB)
  tokenizer.json
  config.json
  special_tokens_map.json
  tokenizer_config.json
```

Source (CORRECTED 2026-07-21 — the old wording caused a real 86 MB mis-ship):
use **`Xenova/all-MiniLM-L6-v2` → `onnx/model_quantized.onnx`** (~22 MB int8),
renamed to `model.onnx` here. Take the tokenizer/config JSONs from either repo.

> ⚠ **Do NOT use `Qdrant/all-MiniLM-L6-v2-onnx` for the weights.** That repo ships
> **fp32 only** (86 MB, no `QuantizeLinear` ops) — there is no quantized file in it.
> This README used to name it first and say "take the quantized `model.onnx`",
> which is impossible from that repo; the fp32 file got bundled and shipped as if
> it were the int8 one, costing ~64 MB of installer for nothing.
>
> Verify any replacement is genuinely int8 before committing to it:
> ```bash
> grep -qa QuantizeLinear model.onnx && echo INT8 || echo "fp32 — WRONG FILE"
> ```
> and then run the loader test, which is the only thing that proves the weights
> actually work (`load_model` fails *silently*, disabling semantic search):
> ```bash
> cargo test --features semantic -- --ignored loads_the_bundled_model
> ```

## Activation steps (build machine)

> **Status 2026-07-16: steps 1–3 are DONE.** This checklist described them as
> future work long after they'd landed, which cost real time — a reader
> following it re-did step 1 as a no-op and mistook step 3 for still-open.
> Kept as a record of what the wiring is, not a to-do list. If you change any
> of it, update this file in the same commit.

1. ~~**Add the dependency**~~ — **DONE.** `src-tauri/Cargo.toml` has
   `fastembed = { version = "4", optional = true, default-features = false,
   features = ["ort-download-binaries"] }` and `semantic = ["dep:fastembed"]`,
   with `semantic` in the default feature set.

   `default-features = false` is load-bearing — fastembed's defaults include
   `hf-hub-native-tls`, which drags in `reqwest` + `native-tls` and statically
   linked a full HTTP/TLS client into the shipped binary, falsifying the About
   screen's "no HTTP client deps in the binary" claim. Do not remove it. Do not
   drop the explicit `ort-download-binaries` either: a bare
   `default-features = false` also disables it and the build breaks.

2. ~~**Bundle the ONNX Runtime DLL**~~ — **NOT NEEDED as configured.**
   `ort-download-binaries` fetches AND LINKS the runtime at build time, so
   there is no `onnxruntime.dll` to ship (confirmed: none appears in the build
   output, and none is in `bundle.resources`). The alternative — vendoring the
   DLL + `ort`'s `load-dynamic` — is still the better call for a fully offline,
   reproducible release, since the current setup needs network **once at build
   time**. That's the one open decision here.

3. ~~**Bundle the model**~~ — **DONE.** All five model files are in
   `tauri.conf.json` → `bundle.resources`, landing under
   `<install>/resources/embedding-runtime/`, which is where `lib.rs` points
   `set_model_dir` via `resource_dir()`.

   **To test in dev:** `tauri dev` resolves the resource dir to `target/debug`,
   which `bundle.resources` does NOT populate — copy or symlink the
   `embedding-runtime/` folder there, or `embedding::is_available()` returns
   false and semantic search stays silently off.

4. **Build with the feature** — nothing to pass; `semantic` is already in
   `default`, so a plain `npm run installer` includes it.

5. **Turn it on**: Search → Advanced indexing → "Semantic search (beta)", then
   rebuild the content index so vectors get written.

## How it behaves once on

- **Index time:** each content-indexed document is split into overlapping
  passages (`embedding::chunk_text`, ~180-word windows, ≤16 per doc), the
  passages are embedded in one batch, and the vectors are stored **int8-
  quantized** (one byte/dimension) in `vector_cache.redb` beside the search
  index. Multi-chunk means a long document's one relevant paragraph is found
  on its own merits instead of being averaged away. Adds real per-file cost —
  that's why it's opt-in.
- **Query time:** the query is embedded, brute-force cosine scores every
  stored chunk, each document is scored by its **best** chunk, and results
  merge with the keyword (BM25/lexical) passes. Semantic hits are weighted
  *below* literal matches (`rank::WEIGHTS.semantic = 0.8` vs `lexical = 3.0`),
  so exact matches always win; semantic surfaces the meaning-related files the
  keywords miss, ranked underneath. Pure-semantic hits show a **"✨ Meaning
  match"** badge in the results.

## Scale & design notes (upgrade paths in code comments)

- **int8 quantization** is the RAM lever (chosen over ANN): 4× less memory
  than f32, exact search kept, negligible accuracy loss. 100 k chunks ≈ 38 MB
  in RAM. Cosine is scale-invariant, so no dequant scale is stored.
  (`vector_cache.rs`)
- **Exact brute-force** cosine over the in-RAM int8 set — correct (not
  approximate). Fine to several hundred k chunks. An ANN index (hnsw) is
  deliberately deferred: it trades exactness for speed and only earns its
  keep past that scale. `top_k`'s signature is stable if it's ever added.
- **Deep pagination:** the semantic candidate count scales with the requested
  page (`offset + limit`, capped at 1000), so meaning-only hits page through
  like keyword hits. (`search.rs`, content query)
- One global model mutex serializes concurrent embed calls across indexer
  threads + queries — per-thread sessions if throughput ever matters.
  (`embedding.rs`)
- Vectors are stored plaintext, consistent with the plaintext content index
  they derive from. Wrap `quantize`/`pack` in DPAPI if the content index ever
  goes encrypted-at-rest. (`vector_cache.rs`)

> **Note:** the store table is `vector_cache_v2` (multi-chunk). If you indexed
> with the earlier single-vector build, **rebuild the content index once** so
> the new multi-chunk store populates.
