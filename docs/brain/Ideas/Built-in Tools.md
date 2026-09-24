---
tags: [searchwin, ideas, tools, privacy]
updated: 2026-09-24
status: proposal
---

# Built-in tools: converters, bangs, voice, clipboard, file search, FishCatcher, file manager, free AI

Back to [[README]] · Related: [[Ideas/Agentic Browser]], [[Roadmap]], [[Architecture]]

> Brainstorm only; nothing here is built yet. It covers the seven tool ideas
> plus "free AI", each checked against the user's own projects for what can be
> reused instead of written from scratch. A 10-agent analysis produced this:
> 8 readers, 1 synthesis, 1 critical review, whose corrections are folded in
> below. Risks and bloat are called out where they're real, especially
> OmniRoute's "free AI" mechanism, which should **not** be copied.

> **Update (2026-09-24):** [[Ideas/KeepItLocal Workspace]] supersedes this
> note's sources for the detector, clipboard, voice and pre-AI redaction, and
> settles the command registry. Read that note first.

## Review findings that change the plan

1. **Search has no phishing protection today.** `Core/Web.cs` passes
   `--disable-features=msSmartScreenProtection,…`, which turns off the one
   protection Edge/WebView2 would give. That makes **FishCatcher a fix for a
   gap we created**, not a nice-to-have. It goes first.
2. **Some WebView2 extension gaps are *confirmed*, not "unverified".**
   `chrome.contextMenus` items never show (`Extensions.AddMenuItems` is empty
   on purpose), `nativeMessaging` has no channel, and manifest `commands` other
   than `_execute_action` are never sent. Any tool that relies on these has to be
   **ported**, not bundled as an extension. `sidePanel` is untested (Search
   doesn't implement it). `alarms`, `notifications` and `downloads` are
   engine-native and probably work, but that's to be tested live.
3. **Voice isn't global yet.** `UI/KeyHook.cs` only acts while Search is the
   foreground window. A push-to-talk key that works from any app needs new
   plumbing (`RegisterHotKey`).
4. **The startup budget.** FishCatcher's tables (~530 KB: PSL, brands,
   ML weights, Bloom filters) must load **lazily, after the first window**, and
   the Bloom filters should ship as raw binary resources, not base64 inside JSON.
5. **Native AOT and user-defined data.** New stored types must be added to the
   source-generated `Json` context. Open-ended user data (custom commands) is
   stored as `JsonNode`, the way `ExtensionManifest` already reads manifests.
6. **PDFs are free.** WebView2 already renders them with Chromium's viewer.
7. **One local index, not two.** File search here and the agent's memory in
   [[Ideas/Agentic Browser]] should be *one* local index with two consumers.
8. **Licence notices.** Copying OmniRoute's free-tier catalog data keeps its
   MIT notice. Bundling FishCatcher's vendored `jsQR.js` carries its Apache-2.0 NOTICE.
9. **The voice app you mentioned wasn't in the list.** None of the six
   projects is it. The only voice code found is KeepItLocal Memory's offline
   **Vosk** STT/TTS. Please point to the dedicated voice app.

---

## 1 · What your projects are

| Project | License | What it actually is | Reusable? |
|---|---|---|---|
| **KeepItLocal AddIns — Excel AI Add-in** (`KeepItLocal\KeepItLocal AddIns\ExcelAddIn`) | No LICENSE file; private/internal | An Excel Office.js task pane + Rust/Axum gateway. The point of the project is governance: the client never sees provider secrets, and every request is classified, PII/secret-redacted, and routed to an allowlisted Databricks or local Ollama host before it leaves. | Not the Excel UI. The Rust gateway's **provider trait + circuit breaker + redaction pipeline** (`providers/`, `security.rs`) is a good *design reference* for the agent's "leash" — bank-specific bits (JWT/OIDC, Databricks, department policy) must be stripped. |
| **KeepItLocal Guard** | UNLICENSED / proprietary, but the user's own | An offline-first Windows DLP: Rust workspace, a privileged service watching the clipboard and files/USB for secrets/PII, a tiered regex+ML detector (~1850 lines), Block/Warn/Redact, a DPAPI-keyed hash-chained audit log, and an optional signed-policy admin server kept strictly separate from the network-free core. | Yes, heavily. **guard-core**'s detector + redactor + policy presets, and **guard-service**'s clipboard listener, are the closest thing to a drop-in for a privacy-respecting clipboard manager and a "sanitize" converter. |
| **KeepItLocal Memory (kip-memory)** | Mixed: engine crate `kip-memory-core` is **MIT**; `semgraphd` (the HTTP daemon / planned AI gateway) is proprietary | A local, encrypted, per-record knowledge graph for AI agents: hybrid vector+FTS5 recall, a "Disclosure Firewall" that redacts PII/secrets per calling AI and signs a receipt for every disclosure, a SemGraph document reader (skeleton-then-zoom), an MCP server, offline Vosk voice (STT+TTS), and an MV3 capture extension scoped to `http://127.0.0.1/*` only. | Yes. MIT engine is a strong match for the agent's memory layer, for offline voice, and — more heavily than its authors intended — for file/content search (SemGraph). The live folder is currently just a `.fastembed_cache`; the real tree is in the sibling zip and needs restoring first. |
| **my-clone** | MIT, but **not the user's project** (JCodesMore's "AI Website Cloner Template", third-party contributors) | An AI-agent skill that reverse-engineers a live website into a Next.js codebase (screenshot → design tokens → per-component specs → parallel builder sub-agents → visual diff). | No direct reuse for browser tooling — it's a dev-time code-gen pipeline, not a runtime feature. The 5-phase *agent orchestration pattern* (spec first, build, diff against ground truth) is worth reading once the Agentic Browser's agent exists, nothing more. |
| **Verifier** (`Projects\authenticator`) | Unknown — no LICENSE, but the user's own private repo | A local-only TOTP/2FA + password vault: Argon2id→XChaCha20-Poly1305 sealed file, optional PIN via DPAPI, QR import via Windows' own `ms-screenclip:`, Google Authenticator bulk-migration QR decode, and a 6-format password-CSV importer. Secrets never leave the PC; no browser credential store is ever read. | Yes, for the small logic pieces: **TOTP generation, QR-via-screenshot, GA migration decode, CSV import** are all small, dependency-light, and already unit-tested — clean ports to C#. The full vault *crypto design* (XChaCha20, Argon2id) has no first-party AOT-friendly .NET equivalent, so that part is borrow-the-design, not copy-the-code. |
| **FishCatcher** | **MIT** (one vendored file, jsQR, is Apache-2.0) — the user's own, shipped v1.0.0 | A cross-browser MV3 extension that scores URLs and page structure for phishing/scam signals, 100% on-device (17 rule-based signals + an on-device ML classifier + Bloom-filter block/allow lists), plus a fully separate, serverless GitHub Actions pipeline that rebuilds and ECDSA-signs a daily threat feed from open community sources. Two optional, off-by-default features (RDAP, Google Safe Browsing) are the only things that ever call out. | Yes, almost entirely. Small, dependency-free JS engine (~1000 lines) with no reflection-shaped code — a clean port target for Native AOT, and the daily signed-feed pipeline needs zero changes to serve a second, C#-based consumer. |
| **OmniRoute** | MIT, but **not the user's project** | Markets itself as "the Free AI Gateway": aggregates 348 providers' free tiers behind one local OpenAI-compatible endpoint. About two-thirds of that is a legitimate aggregator (real, documented, API-key-based free tiers). The rest comes from a `browser-pool` module that runs a stealth headless Chromium to defeat Cloudflare Turnstile / DuckDuckGo's bot check and an Electron login-interceptor that captures OAuth tokens from consumer AI products (Google Antigravity, Claude web, Grok, Qwen, T3 Chat…), replaying them as the official client — including pooling quota across multiple accounts. | Only the idea and the **curated free-tier catalog data** (`FREE_TIERS.md`) as a reference list. The browser-impersonation/token-capture mechanism must not be copied — see [[#8 · Free AI]] below. |
| **AgentsBar** | MIT (portions from CodexBar, MIT) — the user's own, shipped | A Windows tray app + CLI that polls 23 AI-coding-tool APIs for remaining quota/reset time, using CLI session files, pasted API keys (DPAPI-encrypted), or the user's own browser cookies — no server, no telemetry, no new logins. Deliberately refuses to bypass Chromium's v20 app-bound cookie encryption rather than defeat a security control. | Yes, as a design/logic reference: the **polling/backoff/reset-chasing scheduler**, the **DPAPI secrets-at-rest + atomic-write + log-redaction pattern**, and the "reuse an existing session, don't ask for a new login" philosophy all transfer directly. |

---

## 2 · The tools

Each entry: what it does · how it sits in Search · which project feeds it (and how) · privacy · speed · effort · risks.

### 2.1 Converters

- **What it does.** Type an expression in the omnibox, get an instant inline answer, no navigation. Examples: `128 usd to gel`, `1920x1080 to inches @ 96dpi`, `#e07a5f to hsl`, `base64 decode aGVsbG8=`, `jwt decode <token>`, `sha256 of "hello"`, `epoch 1758700000`, `qr encode https://example.com` (renders a QR image inline), `totp JBSWY3DPEHPK3PXP` (live 6-digit code, refreshes), and a **sanitize** mode: `redact <pasted text>` scrubs emails/cards/keys before you paste it somewhere.
- **How.** Omnibox instant-answer path (checked before falling through to web search), Enter or a button copies the result. A `Convert` side-panel doubles as a scratchpad for multi-step conversions.
- **Feeds it.** TOTP/QR from **Verifier**'s `totp.rs` (RFC 6238, already has the RFC test vector) and `import.rs` (screen-capture QR trick). Sanitize mode from **KeepItLocal Guard**'s `redact.rs` + presets, or the smaller `security.rs` in the ExcelAddIn (same idea, less code). Basic unit/base/hash converters are new, small.
- **Reuse strategy:** port to C# (all of the above is small, pure, dependency-light logic — no reflection, easy source-generated JSON for any bundled tables).
- **Privacy.** All local, no network, except currency rates (an optional public rate fetch — no user data in the request, just leaks that *some* Search user asked for a rate, same as any currency widget).
- **Speed budget.** Instant (<5 ms), must not add a network round-trip to the omnibox's normal typing path.
- **Effort.** S for the basic set, M once TOTP/QR/redact are wired in.
- **Risks.** Scope creep toward a full password/2FA manager (that's its own feature, see [[#4 Additional ideas]] item 1) — keep this tool to *stateless* conversions only, no stored secrets.

### 2.2 Bang commands & custom commands

- **What it does.** DuckDuckGo-style bangs and short commands typed in the omnibox: `!yt cats`, `!gh searchwin issues`, `!w rust ownership`, plus Search's own: `!qr <text>`, `!totp <name>`, `!redact <text>`, `!files invoice 2024`, `!clip` (opens clipboard history), and user-defined bangs (`!jira ABC-123` → a URL template the user sets once). A leading `>` runs an action instead of a search: `>close others`, `>new space Work`, `>screenshot`.
- **How.** The omnibox parser checks the first token against the **command registry** (see [[#3 One command surface]]) before treating the input as a search query. Custom bangs are stored the same way bookmarks are (a JSON file), editable from Settings or by typing `!bang add`.
- **Feeds it.** Nothing in the six projects implements this directly — it's the new unifying layer everything else plugs into.
- **Reuse strategy:** new build.
- **Privacy.** Purely local matching; the bang itself may navigate externally (same privacy exposure as typing that URL by hand).
- **Speed budget.** Instant local lookup, must resolve before the omnibox's debounce timer fires a search suggestion.
- **Effort.** S to execute registered bangs, M for a friendly "add your own" UI.
- **Risks.** Ambiguity between a bang and a literal search starting with `!` or `>` — needs an unambiguous prefix and an escape hatch. Any custom command that maps to a **built-in tool action** (not just a URL) must go through the same permission tiers as the agent's leash — a bang shouldn't be a way to bypass "always asks."

### 2.3 Voice commands

- **What it does.** Push-to-talk (a hotkey, reusing Search's existing global keyboard hook) for short commands: "new tab", "go back", "search for rust ownership", "read this page aloud", "summarize this article", "mute this tab", "open my bookmarks". The exact command set should mirror what the user's *other, dedicated voice app* already does — that one is unreviewed here but is "fully theirs," so it's worth reading before designing this from scratch.
- **How.** A visible mic overlay while listening (never ambient/always-on — matches the Agentic Browser note's "visible, not ambient" principle). Recognized text is fed into the **same command registry** as bangs and the agent, not a separate voice-only grammar.
- **Feeds it.** **kip-memory-core**'s `voice.rs`/`speak.rs` — a genuinely offline Vosk STT + TTS pipeline, already wired to a system-wide ask palette in that project — is a concrete, MIT-licensed, working reference. The user's own separate voice app should be reviewed before committing to Vosk, since it may already be tuned for short command-style utterances rather than kip-memory's more conversational use.
- **Reuse strategy:** a sidecar process (bundled Vosk `libvosk.dll` plus a
  small model, or whatever the user's other app already uses). Native AOT can
  call native DLLs, but a sidecar keeps the browser lean and isolated if the
  recognizer crashes.
- **Hotkey.** Inside Search, the existing `KeyHook` works. To work *from any
  app*, register a system hotkey (`RegisterHotKey`), because `KeyHook` ignores
  keys unless Search is in front.
- **Privacy.** Fully offline recognition, audio never leaves the device, no cloud speech API. Mic must be visibly active only while listening.
- **Speed budget.** Model loads lazily only when voice is turned on (zero cost when unused, per the Agentic Browser's principle 5); recognition of a short command should feel near-instant on CPU.
- **Effort.** M once the user's other app's approach is reviewed; L if building STT integration from nothing.
- **Risks.** Vosk models are tens to hundreds of MB, normally downloaded on first use from a third party (alphacephei.com) — needs a bundle-vs-download decision. An always-listening mic is a real privacy smell even if local; push-to-talk (or a very obvious visual state) is the safer default.

### 2.4 Clipboard manager

- **What it does.** History of the last N copies (text, links, images), pinned favorites, search across history, paste-as-plain-text, and an automatic **secret guard**: if you copy or are about to paste something that looks like an API key, card number, password, or other PII, Search warns, offers to redact, or (in a stricter mode) blocks the paste.
- **How.** A side panel or `Ctrl+Shift+V` popup list. A Win32 clipboard-format-listener event (`AddClipboardFormatListener`/`WM_CLIPBOARDUPDATE`), zero polling.
- **Feeds it.** **KeepItLocal Guard**'s `guard-service/monitors/clipboard.rs` is close to a literal template: it already does exactly this (listen, scan, `EmptyClipboard`/overwrite-with-redacted, honor the standard "exclude from clipboard monitoring" format flag password managers set, zero idle CPU). The scan itself uses `guard-core`'s tiered detector.
- **Reuse strategy:** port the listener loop and detector to C#. It's small,
  dependency-light, well-tested Rust. Search's clipboard code today uses the
  WinRT `Clipboard` API (Copy Address, Paste and Go). The history needs a new
  Win32 `AddClipboardFormatListener` on **Search's existing HWND**, not a
  second window or message pump.
- **Privacy.** History stored locally, encrypted at rest (DPAPI, mirroring AgentsBar's `config.rs` pattern), auto-expires, excludes anything flagged "exclude from clipboard monitoring," never transmitted anywhere.
- **Speed budget.** Zero added idle CPU (event-driven, not polling).
- **Effort.** M.
- **Risks.** The clipboard history file is itself a sensitive data store — needs its own encryption and an easy full-wipe. Must not fight Search's existing keyboard hook for the same message pump.

### 2.5 Search files / inside files, open in browser

- **What it does.** Type a filename or a phrase in the omnibox and get local file matches (name and, later, content) alongside tabs/history/bookmarks — e.g. typing "invoice 2024" surfaces a matching PDF from Downloads. Enter opens it **in a Search tab** (a bundled viewer) instead of shelling out to another app.
- **How.** A background indexer, **opt-in per folder** (Downloads,
  Documents; never the whole disk by default). The field searches one unified
  index. **PDFs, images, video, audio, text and HTML already open in a
  WebView2 tab**; Chromium's PDF viewer is built in, so there's no viewer to
  build. Markdown can use the page → Markdown renderer in reverse. Office files
  hand off to their default app at first.
- **One index with the agent's memory.** This is the same local store that
  [[Ideas/Agentic Browser#Memory: yours, on the device, forgettable]] needs.
  Design it once (SQLite FTS5 first, embeddings later), with two consumers.
- **Feeds it.** **kip-memory-core**'s SemGraph engine (Document → Section → chunk tree, "skeleton-then-zoom" progressive reading, backed by `redb` + SQLite FTS5) is the most relevant design and is MIT — though it's built for AI-agent context economy, not "find my file," so it may be more machinery than this needs. A simpler from-scratch indexer on `Microsoft.Data.Sqlite` FTS5 (AOT-friendly) covers filename + basic full-text search without the embedding/graph stack.
- **Reuse strategy:** start with a **new, simple** C# indexer (filename + FTS5 content search); treat kip-memory-core as a **sidecar** option later if "find my file" grows into "let the agent read my files cheaply."
- **Privacy.** Index stays local (optionally encrypted), strictly opt-in per folder, excluded from private/incognito contexts by default.
- **Speed budget.** Indexing runs at idle/background priority; search-as-you-type against the local FTS index should stay under ~50 ms.
- **Effort.** M for filename + basic content search and a PDF viewer; L for a full "index everything, render everything in-tab" version.
- **Risks.** Indexing user files inside a browser process is real scope and real attack surface — a bug could over-read sensitive folders. Needs a clear allowlist UI and a visible "what's indexed" list, not a silent background crawl.

### 2.6 FishCatcher

- **What it does.** Warns (never silently blocks, by design) on phishing/scam sites: brand-typo/homoglyph domains, IP-address hosts, high-abuse TLDs, login pages that post to a foreign domain, tech-support scam pages, crypto seed-phrase requests — using 17 rule-based URL signals plus an on-device ML classifier plus daily-updated block/allow Bloom filters, entirely offline by default.
- **How.** Two integration points: (a) a **NavigationStarting** hook painting an omnibox/toolbar risk indicator before the page even finishes loading; (b) a **WebView2 script injection** (`AddScriptToExecuteOnDocumentCreatedAsync`) doing the DOM-structure probing (password fields, form-action destination, favicon drift) that today's `probe.js` does in the extension.
- **Feeds it.** **FishCatcher** itself, almost entirely: `analyzer.js`/`signals.js`/`psl.js`/`bloom.js`/`ml.js` (~1000 lines, dependency-free, nothing reflection-shaped) for the scoring engine; `probe.js` for the DOM probe; and the **separate, already-running** `fishcatcher-registry` GitHub Actions pipeline for the daily ECDSA-signed threat feed — that pipeline needs *zero changes* to serve a second, C#-based consumer.
- **Reuse strategy:**
  - Port the engine and probe to C# (M).
  - Reuse the signed daily feed as it is, with a small C# ECDSA verifier (S, `System.Security.Cryptography.ECDsa`).
  - **Stopgap:** bundle the existing build as a pre-installed extension (S).
    Worth it, because Search has *no* phishing protection today (SmartScreen is
    off). Known losses: its right-click QR action (`contextMenus` is confirmed
    unsupported) and any keyboard command other than the popup. `sidePanel`
    still needs a live test.
- **Loading:** tables load lazily after the first window, or on the first
  navigation. Bloom filters are raw binary resources.
- **Also decide:** carry over **Family mode** (a larger alert and an optional
  pre-filled help email)?
- **Privacy.** This is FishCatcher's whole reason for existing: zero network calls by default; the two online features (RDAP domain-age check, Google Safe Browsing) are opt-in and off by default; page text/values are never read past small booleans.
- **Speed budget.** Must not add visible latency to navigation — the port's whole appeal is checking *before* the page finishes loading rather than waiting on an extension service worker.
- **Effort.** S (bundle stopgap) → M (native port) → S (feed client).
- **Risks.** Preserve FishCatcher's own "warn, never block" default and "fail open" behavior (every online feature falls back to local/bundled data on any error) — a rewrite could easily lose this by accident and turn a warning tool into a breakage machine.

### 2.7 File manager

- **What it does.** "Windows sucks at it" — a nicer folder view inside a Search tab: navigate, preview, thumbnails, type-ahead search (reusing 2.5's index), and — later, more carefully — rename/move/delete.
- **How.** An internal scheme page (e.g. `search://files/`) as a WinUI panel or a WebView2-hosted UI backed by C# `System.IO` + Windows Shell APIs for thumbnails/icons.
- **Feeds it.** Nothing — none of the six projects is a file manager. This is greenfield; only general WinUI conventions from Search's own codebase carry over.
- **Reuse strategy:** new build.
- **Privacy.** Purely local OS operations. Delete must default to the Recycle Bin (`SHFileOperation`), never a permanent delete without an explicit, separate confirmation — mirroring the fact that permanent deletion is treated as a high-stakes action everywhere else in this stack.
- **Speed budget.** Must feel as instant as Explorer; cache Shell-API thumbnails/icons.
- **Effort.** M for a read-only browse + preview + search v1; L for a full manager with move/delete/rename/network drives.
- **Risks.** **The single biggest scope-creep risk of all seven ideas.** Drag-drop, context menus, permissions, network shares, undo — a full Explorer replacement is a multi-month project on its own. Recommend shipping "a nicer folder view with search" first and deferring risky operations.

### 2.8 Free AI

- **What it does.** Lets Search's planned agent (and the "Ask the page" feature from [[Ideas/Agentic Browser]]) run **without a paid subscription**: the user pastes their own key(s) for providers with genuinely open, documented free tiers (e.g. Gemini, Groq, Mistral, Cerebras, Cloudflare Workers AI, OpenRouter's free models), Search tries them in an ordered fallback, and a local model (Ollama) is the true zero-cost, zero-risk tier underneath.
- **How.** Settings → AI providers: add a key (stored in Credential Manager, same as passwords); a curated provider list (informed by, but re-vetted from, OmniRoute's own `FREE_TIERS.md`) shows what's actually free and ToS-clean today.
- **Feeds it.** **OmniRoute**'s *catalog data and the aggregator pattern only* — one endpoint, ordered fallback, per-provider quota awareness.
- **Reuse strategy:** borrow design + curated allowlist data only.
- **Privacy.** The user's own key, sent only to that provider's official API; no Search-run proxy or server; a "receipt" per request (what was sent, where), matching the Agentic Browser note's principle 1.
- **Speed budget.** Provider-dependent, not a Search-side concern beyond streaming/spinner UX.
- **Effort.** M for a simple, honest router.
- **Risks — read this one carefully.** OmniRoute's headline "~1.51B free tokens/month" number leans heavily on a `browser-pool` module that runs a **stealth headless Chromium** to defeat Cloudflare Turnstile / DuckDuckGo's bot check, and an **Electron login-interceptor** that captures OAuth bearer tokens from consumer AI products (Google Antigravity, Claude web, Grok, Qwen, T3 Chat, Kiro, Coze, Blackbox…) and replays them as if from the official client — including pooling quota across multiple accounts on at least one provider. OmniRoute's own docs tag most of these providers `caution`/`avoid` for exactly this reason, and the router routes to them anyway (the flag is "informational, not a routing gate"). **Do not build this mechanism into Search:**
  - it risks the *user's own accounts* getting banned by third parties,
  - it is a ToS-violation-by-design for a documented subset of providers,
  - it is legally murkier once distributed as a shipped product than as one developer's personal script,
  - and it needs a Node + Next.js + Playwright(+bundled Chromium) + Electron runtime that has nothing to do with a Native AOT WinUI3 app anyway.
  A from-scratch, much smaller C# router that only ever uses the user's own official, permitted credentials is the honest version of "free AI" — smaller numbers, no hidden cost.

---

## 3 · One command surface

Bangs, custom commands, voice, and the agent (from [[Ideas/Agentic Browser]]) should not be four separate parsers. One **command registry** underneath all of them:

```
                     ┌─────────────── Command Registry ───────────────┐
                     │ id · name · aliases/bangs · params · handler ·  │
                     │ permission tier (read / act / consequential)    │
                     └───────┬───────────┬───────────┬────────────────┘
                             │           │           │
        Omnibox (types !bang)│  Voice (says a phrase)│  Agent (calls a tool)
                             │           │           │
                             └─────┬─────┴─────┬─────┘
                                   │           │
                          Bench / MCP server (same commands, over a pipe)
```

- The **omnibox** checks a typed token against the registry before treating it as a search query (§2.2).
- **Voice** maps a recognized phrase to an intent, then calls the *same* registry entry (§2.3) — no separate voice-only grammar to maintain.
- The **agent**'s "tools" (from the Agentic Browser proposal's Hands/leash) are the same registry entries, surfaced with the same permission tiers.
- The **bench** (the existing named-pipe automation server) and any future **MCP server** expose the identical set — `search.convert`, `search.clipboard.history`, `search.files.search`, `search.fishcatcher.status`, `search.files.open` — so a script, Claude Desktop, or the in-browser agent all call the *same* thing.
- Every entry declares its permission tier once, in one place — the Agentic Browser leash table (read / act / always-asks) applies uniformly, so a voice command or a custom bang can never quietly do something the agent itself would have to ask about.
- Native AOT friendly: a static registry built in code (a dictionary filled at
  startup), not discovery through attributes and reflection. **User-defined**
  commands and bangs are stored as `JsonNode` (open-ended, no source-gen type needed).
- **The bench becomes a tool RPC.** Today it only has browser-automation verbs.
  Exposing registry commands over the pipe or MCP is new protocol, with one hard
  rule: **a script can never trigger an always-asks action without a human
  confirming on screen.**
- **Page → Markdown** ([[Ideas/Agentic Browser#Page → Markdown: what the model reads]])
  is itself a registry command: `>copy as markdown`, `>save to obsidian`.

### Speed budgets (checked by the bench)

| Tool | Idle cost | Hot-path budget |
|---|---|---|
| Converters / bangs | 0 | < 5 ms per keystroke in the field, no network |
| FishCatcher | 0 until the first navigation | < 2 ms per `NavigationStarting` check; tables loaded off the UI thread |
| Clipboard manager | 0 CPU (event-driven) | < 10 ms per copy (scan + store) |
| File search | Indexer at idle priority only, off until a folder is added | < 50 ms per query |
| Voice | Nothing loaded until the hotkey | Model load ≤ 1 s; command → action < 300 ms |
| First window | **Must stay ~250 ms**: none of the above loads before it | |

---

## 4 · Additional ideas (grounded in your own projects)

1. **TOTP/2FA + password vault, upgraded.** Fold Verifier's TOTP generation right into the omnibox (`totp <name>`, §2.1), and borrow its Argon2id→XChaCha20-Poly1305 + DPAPI-PIN *design* (not the exact primitives — .NET's BCL has no first-party XChaCha20/Argon2id) to give Search's existing Credential Manager–based storage a proper sealed-vault option.
2. **"Scan this QR" as a general tool**, not just for 2FA — Verifier's `ms-screenclip:` + clipboard-sequence trick, reused for any QR (a page, a photo, a screenshot) to jump straight to its URL.
3. **AI-usage/quota meter** for whichever key the user wires into the agent (§2.8) — port AgentsBar's polling/backoff scheduler, or just shell out to its existing JSON CLI mode.
4. **Tamper-evident "what Shields blocked" log** — KeepItLocal Guard's DPAPI-keyed, hash-chained audit format as a verifiable local record, à la Brave's shield stats but with cryptographic proof nothing was fabricated.
5. **Per-Space privacy profile** — attach a Guard-style policy preset to Search's existing Space concept (a "Work" space auto-redacts secrets on paste; "Personal" only warns), inverting Guard's org-owned control model into a user-owned, easily-toggled one.
6. **Document/text sanitizer** — surfaces Guard's 5 policy presets (General Privacy, Legal Disclosure, HR, Finance, Source Code & Secrets) as selectable "sanitize modes" in the converter tool (§2.1) for anything about to be shared or uploaded.
7. **Encrypted, signed export/import** for bookmarks/history/passwords between machines, with no server — modeled on kip-memory's Ed25519-signed "memory pack" format.
8. **Password-manager import wizard** for onboarding switchers — Verifier's CSV importer already detects Chrome/Edge, Firefox, Bitwarden, 1Password, KeePassXC and Safari/iCloud exports.
9. **Bulk 2FA migration** from a phone authenticator app via Google Authenticator's "transfer accounts" QR — Verifier's hand-rolled protobuf decoder, no extra dependency.
10. **Pre-send redaction for the agent's "Ask the page"** — before any page text goes to a cloud model, run it through the same PII/secret redactor as the clipboard guard (Guard's `guard-core` or the ExcelAddIn's smaller `security.rs`), so "no data leaks" is true everywhere the browser talks to an LLM, not just in one feature.

---

## 5 · Reuse vs. rewrite matrix

| Component | Source project | Strategy | Why | Effort |
|---|---|---|---|---|
| URL phishing/scam scoring engine | FishCatcher | Port to C# | Small, pure, dependency-free, no reflection — clean AOT fit | M |
| FishCatcher DOM probe (AiTM/scam-pack/device-code) | FishCatcher | Port to C# (WebView2 script injection) | Already uses no Chrome-only APIs beyond messaging | M |
| Daily signed threat-list feed | FishCatcher (`fishcatcher-registry`) | Reuse as-is + small C# verifier | Already a decoupled static-JSON+ECDSA pipeline; zero backend changes needed | S |
| FishCatcher (stopgap) | FishCatcher | Bundle as WebView2 extension | Fastest path to real protection on day one | S |
| Clipboard listener + secret guard | KeepItLocal Guard | Port to C# | Small, well-tested Win32 loop + regex detector; must share one message pump with the existing keyboard hook | M |
| PII/secret redaction engine | KeepItLocal Guard (`guard-core`) or ExcelAddIn (`security.rs`) | Port to C# | Dependency-light regex module needed by converters, clipboard, and the agent alike | S |
| Policy presets (sanitize modes, per-space shields) | KeepItLocal Guard | Borrow design | Keyword lists are enterprise-specific; needs consumer-appropriate terms | S |
| Tamper-evident audit log | KeepItLocal Guard | Borrow design, reimplement | DPAPI-keyed hash chain is a good trust feature | S |
| TOTP generation | Verifier | Port to C# | RFC 6238, already has the RFC test vector, tiny | S |
| QR-via-screen-capture import | Verifier | Port to C# | Delegates to `ms-screenclip:`; trivial rewrite with a .NET QR decoder | S |
| Google Authenticator migration decode | Verifier | Port to C# | Hand-rolled protobuf reader, no dependency chain to replicate | M |
| Password CSV import | Verifier | Port to C# | Pure parsing, 15+ unit tests already cover edge cases | M |
| Vault crypto (Argon2id + XChaCha20 + PIN/DPAPI) | Verifier | Borrow design, new .NET primitives | No first-party AOT-friendly XChaCha20/Argon2id in the BCL | XL |
| Local file/document index + progressive read | KeepItLocal Memory (`kip-memory-core`, MIT) | Sidecar (if adopted) | MIT, native, but heavier (embeddings + graph) than "find my file" needs at first | L |
| MV3 capture extension for AI chats | KeepItLocal Memory | Bundle as extension | Small, dependency-free JS, already scoped to `127.0.0.1` only | S |
| Offline voice STT/TTS | KeepItLocal Memory (`voice.rs`/`speak.rs`) + the user's other voice app | Sidecar | Native Vosk binaries — out-of-process is the right call under Native AOT | M |
| Redaction/circuit-breaker pattern for the agent's leash | ExcelAddIn (Rust gateway) | Borrow design | Provider trait + circuit breaker + redaction pipeline maps closely, minus the bank-specific parts | M |
| BYOK free-AI router (legit tiers only) | OmniRoute catalog data (MIT) | Borrow design (curated allowlist) | The aggregator pattern is fine; the browser-pool mechanism is not | M |
| OmniRoute's browser-impersonation / token-capture mechanism | OmniRoute | **Do not reuse** | ToS-violating by its own docs, account-ban risk, wrong runtime for Native AOT | — |
| AI-usage/quota meter | AgentsBar | Port core loop, or shell out to its JSON CLI | Already solves polling/backoff/reset-chasing for 23 providers | M |
| Secrets-at-rest pattern (DPAPI + atomic write + log redaction) | AgentsBar | Borrow design | Clean, already-shipped reference; reimplement via `ProtectedData` | S |
| File manager | — | New build | Greenfield; the single biggest scope-creep risk here | L |
| Local file/content search index (simple version) | — | New build (SQLite FTS5) | Lighter than kip-memory-core for "just find my file" | M |
| Bangs / custom commands / command registry | — | New build | The unifying layer everything else plugs into | M |
| Basic unit/currency/encoding converters | — | New build | Small, standard, no project overlap | S |

---

## 6 · Suggested order

Each phase should be useful on its own and ship without slowing down plain browsing.

| Phase | Ships | Why this order |
|---|---|---|
| **A · Foundation + quick win** | FishCatcher bundled as an extension (stopgap), command registry v1, basic converters, bangs | Closes the phishing gap Search opened by turning off SmartScreen, and builds the shared command surface right away |
| **B · Native, self-contained ports** | FishCatcher native port (replaces the extension), clipboard manager + secret guard, shared PII/secret redaction module | All small, dependency-light Rust/JS ports with no new runtime dependencies |
| **C · Bigger, needs a design call first** | File/content search v1 (filename + SQLite FTS5, no embeddings yet) with a basic PDF viewer; voice commands v1 (push-to-talk, small fixed command set) — after reviewing the user's dedicated voice app | Both need an explicit scope decision before writing code (how much to index; which STT approach) |
| **D · Ties into the Agentic Browser plan** | Free-AI BYOK router (legit tiers only), pre-send redaction wired into "Ask the page" | Feeds directly into [[Ideas/Agentic Browser]]'s Part 2 (Brain, Leash) |
| **E · Larger investment / can wait** | File manager (read-only browse + preview + search first; defer move/delete/rename); TOTP/2FA vault + CSV import + GA migration as a "Vault 2.0" project, once the crypto design question is settled; semantic file search via a kip-memory-core sidecar if simple FTS isn't enough | Each is either high scope (file manager) or needs its own security review (vault crypto) before committing |

---

## 7 · Open questions for the user

- **Where is your voice app?** None of the six projects is it.
- Ship the FishCatcher extension stopgap now (without its right-click QR action), or wait for the native port?
- Keep SmartScreen off (privacy: it sends URLs to Microsoft) and rely on FishCatcher, or offer SmartScreen as an opt-in?
- Which of the seven tools matters most to ship first, given the phase order above?
- Voice: can we see the other app's command list and STT approach before deciding between Vosk-as-sidecar and something else?
- File manager: a full Explorer replacement, or "a nicer folder view with search"? How much file-op risk (move/delete/rename, network drives) should Search actually own?
- Free AI: comfortable limiting this to your own API keys and genuinely open free tiers only — even though that's a far smaller number than OmniRoute's headline claim?
- Clipboard manager: how long should history persist, and is cross-device sync ever wanted (there's no server today), or strictly single-machine?
- File/content search: which folders are indexed by default — opt-in only, or Documents/Downloads pre-checked?
- FishCatcher: keep "warn, never block" as the default in Search too, or add an optional hard-block Shields tier?
- Vault 2.0 (2FA/passwords): extend the existing Credential-Manager-based storage, or move to a new sealed vault file like Verifier's? This decides whether existing saved passwords need a migration path.
- The command registry is the same piece of infrastructure the Agentic Browser proposal's "leash"/tool system needs — build it once, up front, before either bangs or the agent, so neither has to be reworked later?
