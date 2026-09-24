---
tags: [searchwin, ideas, agent, privacy]
updated: 2026-09-24
status: proposal
---

# Search, the agentic browser: ideas

Back to [[README]] · Related: [[Roadmap]], [[Architecture]], [[Windows vs Mac]]

> **North star:** a browser that does things for you the way a trusted
> assistant would: fast, visible, on your side. Nothing leaves the machine
> unless you send it. Brave-grade shields by default, and an agent you can
> watch, stop and trust.

## Principles (the product rules)

1. **Private by construction.** There is still no Search server. The model is
   yours: on the device, or a cloud key you bring. Every time page content goes
   to a model you get a *receipt*: what was sent, where, when.
2. **Visible, not ambient.** The agent works in tabs you can see, in its own
   space, marked with a coloured ring. You can watch, take over, or press `Esc`
   to stop.
3. **Asks before it commits.** Reading is free. Typing is allowed.
   **Submit / buy / send / delete / sign in** always stop and ask.
4. **Pages are data, never instructions.** Text on a web page can't give the
   agent orders. This matters more than any other rule (see [[#Safety against prompt injection]]).
5. **Zero cost when unused.** No model loaded, no script injected and no index
   built until you use them. Browsing speed must not change.
6. **Open and scriptable.** The same actions a person can do, a script can do,
   and so can any agent you trust (MCP).

---

## Part 1 · Shields: Brave-grade blocking and privacy

**Today** (`Core/Shield.cs`): 44 hard-coded tracker domains blocked through
`WebResourceRequested` filters, plus 9 cosmetic selectors, with a per-site pause.
That's clean, but it's nowhere near Brave. In particular, **YouTube ads aren't blocked**,
because YouTube serves them from its own domains.

### How Brave does it, and what we'd do

| Feature | Brave | Search on WebView2 | Effort |
|---|---|---|---|
| Network blocking with real lists (EasyList, EasyPrivacy, uBO lists, regional) | `adblock-rust` engine inside the network stack | **Option A:** built-in, hidden **uBlock Origin Lite** (MV3 declarativeNetRequest runs natively in Chromium's network service, so it's fast). **Option B:** `adblock-rust` compiled to a DLL (it has a C FFI) and called through P/Invoke (AOT-friendly) from `WebResourceRequested`. B is more flexible, but every checked request crosses into our process. **Spike both and measure.** | M |
| **YouTube ads** | Scriptlets from uBO's lists: `json-prune` of `adPlacements`/`playerAds` in the player response, `set-constant`, and fetch/XHR response pruning | Scriptlets injected with `AddScriptToExecuteOnDocumentCreatedAsync` (main world, before page scripts, all frames), which is exactly what that API is for. Lists update **daily** from upstream, since this is an arms race. | M |
| Cosmetic filtering (hide ad slots) | Per-site CSS from the lists | Generic + per-site CSS injected at document start. Reuses the **Curtain** machinery | S |
| Cookie banners | "Block cookie consent notices" | The EasyList Cookie list plus a small auto-reject script ("reject all" where possible, never "accept") | S |
| Tracking parameters | Query-string filtering | Strip `utm_*`, `fbclid`, `gclid`, `mc_eid`… from navigations (`NavigationStarting`, re-navigate cleaned) and from **Copy Address** | S |
| Bounce tracking | Debouncing | Unwrap known redirectors (`l.facebook.com`, `t.co`…, and Google's `/url?q=`) before navigating | S |
| AMP | De-AMP | Detect an AMP page and go to its canonical URL | S |
| HTTPS | HTTPS by default | An HTTPS-first upgrade with a fallback prompt | S |
| Fingerprinting | "Farbling" inside the engine | **Honest limit:** WebView2 can't be patched, so JS-level noise on canvas/audio/WebGL is weaker and detectable. Offer a *strict* mode, not a promise. Keep tracking prevention at Balanced (Strict breaks sign-ins) | M, partial |
| Forget on close | Per-site "forget me" | Clear a site's cookies and storage when its last tab closes (`CookieManager` + `Profile.ClearBrowsingDataAsync`). Per site, or a whole *forgetful space* | S |
| Shields panel | Lion icon with counts | Keep the **quiet** Mac philosophy (no green shield), but add a small per-site sheet: blocked count, *pause here*, *strict here*, *report breakage* | S |
| Tor windows | Built in | Later, maybe. It needs a bundled Tor and a proxy environment of its own. Out of scope for now | L |

**Filter list updates** need a small signed-update channel, or fetching
straight from the list publishers daily (that leaks nothing but your IP to
them). Check the licences: the uBO lists and EasyList are GPLv3 / CC BY-SA.
Download them at runtime rather than bundling them.

**Speed budget:** blocking must make pages *faster*. Add the blocked count and
bytes saved to the bench, and measure page load with and without shields.

---

## Part 2 · The agent: architecture

We already own most of the parts. **The bench is the agent's hands.**

```
            ┌───────────── You (field, side panel, voice) ─────────────┐
            │                                                           │
   Brain  ──┤  Planner (model you chose)  ←→  Tools  ←→  Leash (permissions)
            │                                  │
   Eyes   ──┤  Reader text · AX tree · screenshot (only when needed)
   Hands  ──┤  open · click · type · select · scroll · wait · tabs   ← Bench.cs today
   Memory ──┤  local index of history, tabs, notes (opt-in, on device)
            └───────────────────────────────────────────────────────────┘
```

### Eyes: see pages cheaply and precisely
- **Reader text** (`Core/Reader.cs` already extracts it). This is the default
  context: small, clean and cheap.
- **Accessibility tree** through CDP `Accessibility.getFullAXTree`
  (`CallDevToolsProtocolMethodAsync`), giving roles, names and states. The
  agent acts on *elements* ("button 'Add to cart'"), not pixels. That's faster,
  more reliable and needs fewer tokens than screenshots.
- **Screenshot** only when the tree isn't enough (canvas apps, maps). The
  bench's `shot` already does this.

#### Page → Markdown: what the model reads
Markdown is the best format to hand a model a page: it keeps the structure
(headings, lists, tables, links) and uses far fewer tokens than HTML. Two
projects were checked (2026-09-24):

| | [markdowner](https://github.com/supermemoryai/markdowner) | [html-to-markdown](https://github.com/JohannesKaufmann/html-to-markdown) |
|---|---|---|
| What | A *hosted service*: Cloudflare Workers + Cloudflare's remote Browser Rendering + **Turndown** (JS) + KV cache; optional LLM clean-up; crawls up to 10 subpages | A Go library and CLI (v2): HTML → Markdown only, no content extraction. Plugins for GFM tables (rowspan/colspan), strikethrough, absolute URLs |
| License | MIT | MIT |
| Fit for Search | **Take the idea, not the service.** It sends the URL to a server that fetches the page again: a privacy leak, it can't see signed-in pages, and it's slower. We *are* the browser and already have the rendered DOM. What's worth reusing is **Turndown** (MIT, ~30 KB JS) running *inside the page* | Better table handling than Turndown. It would need a Go `c-shared` DLL (P/Invoke, AOT-friendly, but adds the Go runtime, a few MB) or a CLI sidecar, and the HTML has to cross from the page into our process. **Keep it as the fallback** if Turndown's output falls short |

**Plan:** `Reader.cs`'s in-page article detection picks the content → **visible
elements only** (drop anything `display:none`, `visibility:hidden`, zero-size or
off-screen: hidden text is a common prompt-injection trick) → Turndown +
turndown-plugin-gfm in the page, injected on demand (never on page load) →
Markdown with absolute URLs → the model. Zero network, and it works on signed-in pages.

The same pipeline gives:
- **Copy page as Markdown**
- **Save to Obsidian**, with a front-matter source and date
- research-mode notes
- the text for local semantic history (KeepItLocal Memory?)
- **Crawl subpages** as markdowner does, done locally with sleeping tabs in the agent space

### Hands: act like a person, through the page's own events
Reuse the bench's `open, go, click, tap, type, key, select, submit, wait, tabs,
text, shot, close`. Add `scroll`, `hover`, `upload` (always asks) and
`extract(schema)`. Every action is logged in a **step list** you can read,
undo where possible, and replay.

### Brain: bring your own model
- **Local:** Ollama or LM Studio (`localhost`), or Windows' on-device model
  (Phi Silica through the Windows AI APIs on Copilot+ PCs, installed on demand,
  never bundled; see [[Decisions#D5 · Windows App SDK *component* packages, not the metapackage]]).
- **Cloud with your own key:** Claude, OpenAI, Gemini, Mistral, stored in
  Credential Manager like passwords.
- **Mix:** a small local model for cheap work (classify, summarize, redact)
  and a big model only for planning, if you allow it.
- No account and no Search proxy. Streaming, token and cost meter per task.

### Leash: permissions you can understand
| Action | Default |
|---|---|
| Read the current page / the tabs you pick | Allowed while you ask |
| Open tabs, navigate, scroll, type into fields | Allowed inside the agent's task |
| **Submit forms, buy, send, post, delete, accept terms** | **Always asks**, showing exactly what will be sent |
| Sign in | The **vault fills passwords itself**; the model never sees them |
| Downloads and uploads | Asks, naming the file |
| Move data between sites (copy from site A into site B) | Asks the first time for each pair of sites |
| Payment fields, government IDs | Never. You do these yourself |

Grants can be per site and per task, and they expire when the task ends.

### Memory: yours, on the device, forgettable
- **Semantic history** (opt-in): a local embedding index of the text of pages
  you visited, so you can ask "that article on Rust async from last week". It's
  built only for pages you read for more than N seconds, excludes private tabs
  and banking, and can be erased per site.
- **Per space:** Work memory never leaks into Personal.
- **One index, shared with file search** ([[Ideas/Built-in Tools]] §2.5).
  KeepItLocal Memory's MIT engine (`kip-memory-core`) is the candidate once
  simple FTS5 isn't enough.
- **Export** to Markdown (and Obsidian): research notes with citations.

---

## Part 3 · Features people would love, roughly in order

1. **Ask the page** (`Ctrl+E`, or type `?` in the field): summarize, explain,
   "what's the catch?" on a long article, terms of service or PR. Works
   **offline** with a local model.
2. **YouTube, but better:** no ads (Part 1), plus "summarize this video" and
   "jump to where they talk about X" from the transcript.
3. **Ask across tabs:** "compare these three laptops" builds a table from the
   open tabs, with a citation per cell. This is where tabs and spaces pay off.
4. **Do it for me, while I watch:** "find a flight to Tbilisi under €200 on the
   14th". The agent works in the **Agent space**, you see the tabs move, and it
   stops at the booking page with everything filled in for your **confirm**.
5. **Research mode:** the agent reads 10–20 sources in the background (asleep
   tabs cost nothing), then writes a cited brief you can save to Obsidian.
6. **Watchers:** "tell me when this is back in stock / under €500 / this page
   changes". It checks with a local diff on a schedule, and costs no model call
   unless something changes.
7. **Recipes** (record and replay): do something once, and Search turns it into
   a parameterised recipe ("download my monthly invoice from X"). It runs on
   demand or on a schedule, and any step marked *submit* still asks. Recipes are
   bench scripts, so they're shareable and reviewable text.
8. **Tab cleanup:** "close what I'm done with and group the rest into spaces".
   The plan is shown first, one click applies it, and it can be undone.
9. **Form filling from your profile:** address and contacts from a local card,
   never from the model's memory.
10. **Translate and read aloud** locally: Reader mode + a local model + Windows' voices.
11. **Search as an MCP server:** Claude Desktop, Claude Code or any agent can
    use *your* browser (with your sign-ins) **only if you allow it, per task, with
    the same leash and the ring visible**. The bench already speaks most of this.
    Almost no other browser offers this cleanly, so it's a real differentiator.
12. **Search as an MCP client:** the agent can use your other tools (calendar,
    notes, files) when you connect them. For example, "put this event in my calendar".

---

## Safety against prompt injection

The biggest risk for agentic browsers. The design rules:

- **Two channels:** your instructions go to the planner as instructions. Page
  content arrives wrapped and labelled as untrusted data, and it can never
  widen permissions.
- **Consequential actions go through the leash, whatever the model says.** A
  hijacked plan still hits a confirmation that shows the *real* target and payload.
- **Cross-site data flow is gated.** Reading your mail and then pasting it into
  another site asks first.
- **Sensitive-site guard:** banks, mail, password pages and admin consoles are
  read-only for the agent unless you explicitly allow more.
- **The agent space starts signed out** by default. Giving it your sign-ins is a
  per-task choice.
- **An audit log** of every step, locally, that you can scroll back through.
- **Red-team suite:** a folder of hostile test pages that the bench runs against
  every build.

---

## Part 4 · Phased plan

| Phase | What ships | Builds on |
|---|---|---|
| **0 · Spikes (1–2 wks)** | Measure uBO Lite vs adblock-rust on WebView2; check YouTube scriptlets; AX tree through CDP; Ollama round-trip | Shield, Tab, Bench |
| **1 · Shields 2.0** | Real lists + daily updates, YouTube, cookie banners, parameter stripping, debounce, forget-on-close, per-site sheet | `Shield.cs`, `Curtain.cs` |
| **2 · Ask** | Models setting (local/cloud, key in Credential Manager), Ask the page, video summary, compare tabs, context receipts | `Reader.cs`, Settings, Vault |
| **3 · Act** | Agent space + ring, the tools (bench → agent API), the leash, step log, `Esc` stops everything | `Bench.cs`, Spaces |
| **4 · Remember and repeat** | Local semantic history, watchers, recipes, Obsidian export | History, Store |
| **5 · Open** | MCP server/client, shareable recipes, the red-team suite in CI | Bench protocol |

Each phase is useful on its own, and none of them makes plain browsing slower.

---

## How it compares

- **Brave:** great shields; Leo chat; agentic browsing still early and kept
  separate. *Ours:* the same shields on Windows, in a much quieter UI, with the
  agent built around your own model and a visible leash.
- **Comet / Atlas / Edge Copilot Mode / Dia:** strong agents, but the model and
  the memory live on *their* servers. *Ours:* no server at all, and local-first.
- **Arc-style spaces plus an agent space** is a natural fit: the agent gets its
  own lane.
- **The MCP server** turns Search into the browser *other* agents use safely.

## Open questions

- [ ] Bundle uBO Lite (GPLv3) or embed adblock-rust (MPL-2.0)? Decide after the spike.
- [ ] Where do the list updates come from? This ties into [[Roadmap]] (signing + update channel).
- [ ] Minimum hardware for a useful local model, and what the default is when there isn't one.
- [ ] Accessibility of the agent UI (announce steps to screen readers).
- [ ] Does YouTube's anti-adblock wall need an answer beyond the upstream lists?
