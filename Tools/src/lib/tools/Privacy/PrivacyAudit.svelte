<script lang="ts">
    import { onMount } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import { Button } from '$lib/ui';
    import { toast } from '$lib/stores/toasts';
    import { errorToast } from '$lib/stores/errorToast';
    import {
        acknowledged,
        acknowledge,
        unacknowledge,
        initPrivacyAcks,
    } from '$lib/stores/privacyAck';
    import {
        ShieldCheck,
        ShieldAlert,
        Mic,
        Puzzle,
        Rocket,
        Globe,
        ScanLine,
        RefreshCw,
        WifiOff,
        CircleCheck,
        Check,
        RotateCcw,
        ChevronDown,
        ExternalLink,
        FolderOpen,
        Network,
        FolderGit2,
        CalendarClock,
        KeyRound,
        Eye,
        EyeOff,
        type Icon as LucideIcon,
    } from '@lucide/svelte';
    import PrivacyBlur from '$lib/components/PrivacyBlur.svelte';
    import { sensitiveKindLabel } from '$lib/types/sensitive';
    // Scan state lives in a module store so a finished audit survives leaving
    // and returning to this tool (the workspace re-mounts the component on
    // navigation, wiping any component-local $state). Acks stay in privacyAck.
    import {
        results,
        ranOnce,
        scanning,
        errorMsg,
        openSections,
        previewOpen,
        previewCache,
        runPrivacyAuditScan,
        type Finding,
        type FindingFix,
        type FilePreview,
    } from '$lib/stores/privacyAudit';

    // The scans to run, in display order. Each maps to one backend command
    // returning a ScanResult.
    // Display metadata per scan (icon + title), keyed by scan id. The dispatch
    // list — which backend command each id runs — is the single source of truth
    // in stores/privacyAudit.ts (PRIVACY_SCANS), shared with the opt-in
    // scheduler. Keep the two id sets in sync. (audit_unencrypted_pii reuses the
    // search index's sensitive-content tags — no extra file walk.)
    const SCANS: { id: string; title: string; icon: typeof LucideIcon }[] = [
        { id: 'mic-camera', title: 'Microphone & Camera', icon: Mic },
        { id: 'browser-extensions', title: 'Browser Extensions', icon: Puzzle },
        { id: 'startup-programs', title: 'Startup Programs', icon: Rocket },
        { id: 'scheduled-tasks', title: 'Scheduled Tasks', icon: CalendarClock },
        { id: 'outbound-connections', title: 'Outbound Connections', icon: Globe },
        { id: 'hosts-file', title: 'Hosts File', icon: Network },
        { id: 'dev-secrets', title: 'Exposed .env & Keys', icon: FolderGit2 },
        { id: 'unencrypted-pii', title: 'Unencrypted Secrets in Indexed Files', icon: KeyRound },
    ];

    // Sections are expanded by scan id. Default: only sections that need
    // attention (or carry guidance) open — that decision is made per-scan as
    // each result streams in (see runScan). This just toggles one open/closed.
    function toggleSection(id: string) {
        openSections.update((o) => ({ ...o, [id]: !o[id] }));
    }

    /** Toggle the Preview panel for a finding. On first open, fetch the
     *  file's content + findings from the backend and cache. */
    async function togglePreview(f: Finding) {
        if (!f.previewPath) return;
        const open = !$previewOpen[f.id];
        previewOpen.update((p) => ({ ...p, [f.id]: open }));
        if (!open) return;
        const cached = $previewCache[f.id];
        if (cached && cached !== 'loading') return;
        previewCache.update((c) => ({ ...c, [f.id]: 'loading' }));
        try {
            const preview = await invoke<FilePreview>('preview_file_findings', {
                path: f.previewPath,
                includeLow: false,
            });
            previewCache.update((c) => ({ ...c, [f.id]: preview }));
        } catch (e) {
            previewCache.update((c) => ({
                ...c,
                [f.id]: {
                    excerpts: [],
                    totalFindings: 0,
                    truncated: false,
                    error: String(e),
                },
            }));
        }
    }

    onMount(() => {
        void initPrivacyAcks();
    });

    async function runScan() {
        // Single shared scan path (stores/privacyAudit.ts) — also used by the
        // opt-in scheduler, so a scheduled run and a manual click populate the
        // same stores. Per-scan errors + the concurrent-run guard live in the
        // runner; the page just triggers it.
        await runPrivacyAuditScan();
    }

    async function runFix(fix: FindingFix | undefined) {
        if (!fix) return;
        try {
            if (fix.action === 'open-setting') {
                await invoke('open_privacy_setting', { target: fix.target });
            } else if (fix.action === 'reveal-file') {
                await invoke('reveal_in_explorer', { path: fix.target });
            } else if (fix.action === 'reveal-path') {
                await invoke('open_search_result_path', { path: fix.target });
            }
        } catch (e) {
            errorToast("Couldn't open that location", e, {
                hint: 'The file or folder may have been moved or deleted since the scan.',
            });
        }
    }

    let ackedOpen = $state(false);
    // Acknowledged findings drop out of the active list + the score, so a
    // re-run only surfaces what's new or still unaddressed.
    let ackSet = $derived(new Set($acknowledged));
    let allFindings = $derived($results.flatMap((r) => r.findings));
    let activeFindings = $derived(allFindings.filter((f) => !ackSet.has(f.id)));
    let ackedFindings = $derived(allFindings.filter((f) => ackSet.has(f.id)));

    // Privacy score (frontend heuristic over the ACTIVE findings). Only real
    // problems move it: high = a private key on disk / something recording now;
    // medium = a password file, a broad-permission extension. INFO findings
    // (an app merely *having* mic permission, a program with a normal internet
    // connection) are awareness items, not problems — they don't dock points.
    // Acknowledging a finding you accept also raises the score. Caps [0, 100].
    let score = $derived.by(() => {
        if (activeFindings.length === 0) return 100;
        let s = 100;
        for (const f of activeFindings) {
            s -=
                f.severity === 'high'
                    ? 18
                    : f.severity === 'medium'
                      ? 10
                      : f.severity === 'low'
                        ? 5
                        : 0;
        }
        return Math.max(0, Math.min(100, s));
    });
    let scoreTone = $derived(score >= 80 ? 'good' : score >= 50 ? 'warn' : 'bad');

    // "Active now" is mic/camera-specific — a high-severity finding there
    // means a device is open right this second (acknowledged ones excluded).
    let activeNow = $derived(
        $results
            .find((r) => r.scan === 'mic-camera')
            ?.findings.filter((f) => f.severity === 'high' && !ackSet.has(f.id)).length ?? 0,
    );

    function fixIcon(action: string) {
        return action === 'open-setting' ? ExternalLink : FolderOpen;
    }
</script>

<div class="pa-root">
    <header class="pa-head">
        <div class="pa-head-main">
            <div class="pa-head-ico"><ShieldCheck /></div>
            <div>
                <h1>Privacy Audit</h1>
                <p>
                    Local checks of what can see, hear, or read you on this machine — with a
                    one-click fix for each finding.
                </p>
            </div>
        </div>
        <div class="pa-head-actions">
            <span class="pa-local-chip" title="Every scan runs entirely on this device.">
                <WifiOff class="pa-chip-ico" /> 100% local · no network
            </span>
            {#if $ranOnce}
                <Button variant="secondary" icon={RefreshCw} loading={$scanning} onclick={runScan}>
                    Re-scan
                </Button>
            {/if}
        </div>
    </header>

    {#if !$ranOnce}
        <!-- First-run hero: a single, deliberate call to action. Nothing has
             scanned yet — we never run automatically. -->
        <section class="pa-hero">
            <div class="pa-hero-ico"><ScanLine /></div>
            <h2>Run your first privacy scan</h2>
            <p>
                KeepItLocal checks which apps can use your
                <strong>microphone &amp; camera</strong>, which
                <strong>browser extensions</strong> hold broad permissions, what
                <strong>runs at startup</strong>, and whether
                <strong>.env files or private keys</strong> are sitting exposed in your projects.
                Nothing leaves your device, and nothing runs until you press the button.
            </p>
            <Button variant="primary" icon={ScanLine} loading={$scanning} onclick={runScan}>
                {$scanning ? 'Scanning…' : 'Run privacy scan'}
            </Button>
            <p class="pa-hero-foot">
                It also lists which programs are <strong>connected to the internet</strong> right
                now. Every check runs on this device — no accounts, no uploads, no background
                scanning.
            </p>
        </section>
    {:else if $errorMsg}
        <section class="pa-state pa-state-error">
            <ShieldAlert class="pa-state-ico" />
            <div>
                <h2>Scan couldn't finish</h2>
                <p>{$errorMsg}</p>
            </div>
        </section>
    {:else}
        <!-- Score + summary -->
        <section class="pa-score-card pa-tone-{scoreTone}">
            <div class="pa-score-num">
                <span class="pa-score-value">{score}</span>
                <span class="pa-score-max">/100</span>
            </div>
            <div class="pa-score-text">
                <div class="pa-score-title">
                    {#if scoreTone === 'good'}Looking good{:else if scoreTone === 'warn'}Worth a
                        review{:else}Needs attention{/if}
                </div>
                <div class="pa-score-summary">
                    {activeFindings.length === 0
                        ? 'Nothing flagged across all checks.'
                        : `${activeFindings.length} item${activeFindings.length === 1 ? '' : 's'} flagged across ${$results.length} checks.`}
                </div>
            </div>
        </section>

        {#if activeNow > 0}
            <div class="pa-active-banner">
                <ShieldAlert class="pa-active-ico" />
                <span>
                    {activeNow}
                    {activeNow === 1 ? 'app is' : 'apps are'} using your microphone or camera
                    <strong>right now</strong>.
                </span>
            </div>
        {/if}

        {#each $results as result (result.scan)}
            {@const meta = SCANS.find((s) => s.id === result.scan)}
            {@const SectionIcon = meta?.icon ?? ShieldCheck}
            {@const visible = result.findings.filter((f) => !ackSet.has(f.id))}
            {@const ackedHere = result.findings.length - visible.length}
            {@const worst = visible.some((f) => f.severity === 'high')
                ? 'high'
                : visible.some((f) => f.severity === 'medium')
                  ? 'medium'
                  : visible.some((f) => f.severity === 'low')
                    ? 'low'
                    : 'info'}
            {@const open = $openSections[result.scan] ?? false}
            <section class="pa-group" class:is-open={open}>
                <button
                    type="button"
                    class="pa-group-head"
                    onclick={() => toggleSection(result.scan)}
                    aria-expanded={open}
                >
                    <SectionIcon class="pa-group-ico" />
                    <h3>{meta?.title ?? result.scan}</h3>
                    <span class="pa-group-status">
                        {#if result.loading}
                            <span class="pa-status pa-status-muted pa-status-loading">
                                <RefreshCw class="pa-status-spin" /> Scanning…
                            </span>
                        {:else if result.unavailable}
                            <span class="pa-status pa-status-muted">Unavailable</span>
                        {:else if visible.length === 0}
                            <span class="pa-status pa-status-clear">
                                <CircleCheck class="pa-status-ico" /> Clear
                            </span>
                        {:else}
                            <span class="pa-status pa-status-flag pa-sev-{worst}">
                                <span class="pa-dot" aria-hidden="true"></span>
                                {visible.length} to review
                            </span>
                        {/if}
                        <ChevronDown class="pa-group-chev {open ? 'is-open' : ''}" />
                    </span>
                </button>
                {#if open}
                    <div class="pa-group-body">
                        {#if result.summary}
                            <p class="pa-group-summary">{result.summary}</p>
                        {/if}

                {#if result.loading}
                    <div class="pa-clear-row">
                        <RefreshCw class="pa-status-spin" />
                        <span>Scanning…</span>
                    </div>
                {:else if result.unavailable}
                    <div class="pa-soft-note">
                        <ShieldAlert class="pa-soft-ico" />
                        <p>This check isn't available on this system.</p>
                    </div>
                {:else if visible.length === 0}
                    <div class="pa-clear-row">
                        <CircleCheck class="pa-clear-ico" />
                        <span>
                            {ackedHere > 0
                                ? `Nothing left to review (${ackedHere} acknowledged).`
                                : 'Nothing flagged.'}
                        </span>
                    </div>
                {:else}
                    <div class="pa-list">
                        {#each visible as f (f.id)}
                            {@const FixIcon = fixIcon(f.fix?.action ?? '')}
                            {@const previewState = $previewCache[f.id]}
                            {@const isPreviewOpen = !!$previewOpen[f.id]}
                            <article class="pa-card pa-sev-{f.severity}">
                                <span class="pa-dot" aria-hidden="true"></span>
                                <div class="pa-card-body">
                                    <div class="pa-card-top">
                                        <span class="pa-card-title">{f.title}</span>
                                        {#if f.severity === 'high'}
                                            <span class="pa-tag">
                                                {result.scan === 'mic-camera'
                                                    ? 'Active now'
                                                    : 'High risk'}
                                            </span>
                                        {/if}
                                    </div>
                                    {#if f.kinds && f.kinds.length > 0}
                                        <!-- Phase 6.5-5: per-finding kind chips.
                                             Show WHAT was found at a glance ("AWS
                                             access key", "OpenAI API key") instead
                                             of burying it in the detail text. -->
                                        <div class="pa-kinds">
                                            {#each f.kinds.filter((k) => k !== 'sensitive') as kind (kind)}
                                                <span class="pa-kind-chip">
                                                    {sensitiveKindLabel(kind)}
                                                </span>
                                            {/each}
                                        </div>
                                    {/if}
                                    <div class="pa-card-detail">{f.detail}</div>
                                    <div class="pa-card-rec">{f.recommendation}</div>

                                    {#if isPreviewOpen}
                                        <!-- Phase 6.5-5 (excerpts revision):
                                             show ONLY the matched lines + small
                                             context window, not the whole file.
                                             Each excerpt renders through
                                             PrivacyBlur with its own line-number
                                             header so the user can locate the
                                             hit in the original file. -->
                                        <div class="pa-preview">
                                            {#if previewState === 'loading'}
                                                <div class="pa-preview-loading">
                                                    Loading preview…
                                                </div>
                                            {:else if previewState && previewState.error}
                                                <div class="pa-preview-error">
                                                    Couldn't load: {previewState.error}
                                                </div>
                                            {:else if previewState}
                                                {#if previewState.excerpts.length === 0}
                                                    <div class="pa-preview-empty">
                                                        File no longer contains
                                                        detectable secrets — it
                                                        may have been edited
                                                        since the index was built.
                                                    </div>
                                                {:else}
                                                    <div class="pa-preview-meta">
                                                        {previewState.totalFindings} match{previewState.totalFindings === 1 ? '' : 'es'}
                                                        across {previewState.excerpts.length} excerpt{previewState.excerpts.length === 1 ? '' : 's'}
                                                    </div>
                                                    {#each previewState.excerpts as ex (ex.lineStart)}
                                                        <div class="pa-excerpt">
                                                            <div class="pa-excerpt-head">
                                                                Line {ex.lineStart}
                                                            </div>
                                                            <div class="pa-excerpt-body">
                                                                <PrivacyBlur
                                                                    text={ex.content}
                                                                    findings={ex.findings}
                                                                    blurTiers={[
                                                                        'high',
                                                                        'medium',
                                                                    ]}
                                                                    showCopy={false}
                                                                />
                                                            </div>
                                                        </div>
                                                    {/each}
                                                {/if}
                                                {#if previewState.truncated}
                                                    <div class="pa-preview-trunc">
                                                        File scanned up to 64 KB only — there may be more matches further down.
                                                    </div>
                                                {/if}
                                            {/if}
                                        </div>
                                    {/if}
                                </div>
                                <div class="pa-card-actions">
                                    {#if f.previewPath}
                                        <button
                                            type="button"
                                            class="pa-fix"
                                            onclick={() => void togglePreview(f)}
                                            title={isPreviewOpen
                                                ? 'Close preview'
                                                : 'Preview the file with matches highlighted'}
                                        >
                                            {#if isPreviewOpen}
                                                <EyeOff class="pa-fix-ico" />
                                                Hide preview
                                            {:else}
                                                <Eye class="pa-fix-ico" />
                                                Preview
                                            {/if}
                                        </button>
                                    {/if}
                                    {#if f.fix}
                                        <button
                                            type="button"
                                            class="pa-fix"
                                            onclick={() => runFix(f.fix)}
                                        >
                                            <FixIcon class="pa-fix-ico" />
                                            {f.fix.label}
                                        </button>
                                    {/if}
                                    <button
                                        type="button"
                                        class="pa-ack"
                                        title="Acknowledge — I know about this; hide it from future scans"
                                        onclick={() => acknowledge(f.id)}
                                    >
                                        <Check class="pa-ack-ico" />
                                        OK, I know
                                    </button>
                                </div>
                            </article>
                        {/each}
                    </div>
                        {/if}
                    </div>
                {/if}
            </section>
        {/each}

        {#if ackedFindings.length > 0}
            <section class="pa-acked">
                <button
                    type="button"
                    class="pa-acked-toggle"
                    onclick={() => (ackedOpen = !ackedOpen)}
                    aria-expanded={ackedOpen}
                >
                    <Check class="pa-acked-ico" />
                    <span>Acknowledged ({ackedFindings.length})</span>
                    <ChevronDown class="pa-acked-chev {ackedOpen ? 'is-open' : ''}" />
                </button>
                {#if ackedOpen}
                    <div class="pa-list pa-acked-list">
                        {#each ackedFindings as f (f.id)}
                            <article class="pa-card pa-acked-card">
                                <div class="pa-card-body">
                                    <span class="pa-card-title">{f.title}</span>
                                    <div class="pa-card-detail">{f.detail}</div>
                                </div>
                                <button
                                    type="button"
                                    class="pa-ack"
                                    title="Show this again"
                                    onclick={() => unacknowledge(f.id)}
                                >
                                    <RotateCcw class="pa-ack-ico" />
                                    Restore
                                </button>
                            </article>
                        {/each}
                    </div>
                {/if}
            </section>
        {/if}
    {/if}
</div>

<style>
    .pa-root {
        padding: 24px 28px 48px;
        max-width: 880px;
        /* Left-aligned with the heroes (matches the other tool pages), not
           centered in the content area. */
        margin: 0;
    }

    /* ── Header ── */
    .pa-head {
        display: flex;
        align-items: flex-start;
        justify-content: space-between;
        gap: 16px;
        flex-wrap: wrap;
    }
    .pa-head-main {
        display: flex;
        gap: 14px;
        align-items: flex-start;
    }
    .pa-head-ico {
        flex: none;
        width: 40px;
        height: 40px;
        display: grid;
        place-items: center;
        border-radius: 12px;
        color: var(--color-accent);
        box-shadow:
            inset 0 1px 0 color-mix(in srgb, var(--color-text) 10%, transparent),
            inset 0 0 0 1px color-mix(in srgb, var(--color-text) 5%, transparent);
    }
    .pa-head-ico :global(svg) {
        width: 22px;
        height: 22px;
    }
    .pa-head h1 {
        margin: 0;
        font-size: 21px;
        font-weight: 700;
        color: var(--color-text);
    }
    .pa-head p {
        margin: 3px 0 0;
        font-size: 13px;
        color: var(--color-text-secondary);
        max-width: 52ch;
    }
    .pa-head-actions {
        display: flex;
        align-items: center;
        gap: 10px;
        flex-wrap: wrap;
    }
    .pa-local-chip {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        font-size: 11.5px;
        font-weight: 600;
        color: var(--color-text-secondary);
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        padding: 5px 10px;
        border-radius: 999px;
    }
    .pa-local-chip :global(.pa-chip-ico) {
        width: 13px;
        height: 13px;
        color: var(--color-accent);
    }

    /* ── First-run hero ── */
    .pa-hero {
        margin-top: 32px;
        display: flex;
        flex-direction: column;
        align-items: flex-start;
        text-align: left;
        gap: 12px;
        padding: 40px 28px;
        background: var(--color-panel);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card, 14px);
    }
    .pa-hero-ico {
        width: 56px;
        height: 56px;
        display: grid;
        place-items: center;
        border-radius: 16px;
        color: var(--color-accent);
        box-shadow:
            inset 0 1px 0 color-mix(in srgb, var(--color-text) 10%, transparent),
            inset 0 0 0 1px color-mix(in srgb, var(--color-text) 5%, transparent);
    }
    .pa-hero-ico :global(svg) {
        width: 30px;
        height: 30px;
    }
    .pa-hero h2 {
        margin: 4px 0 0;
        font-size: 18px;
        font-weight: 700;
        color: var(--color-text);
    }
    .pa-hero p {
        margin: 0;
        font-size: 13.5px;
        color: var(--color-text-secondary);
        max-width: 54ch;
        line-height: 1.55;
    }
    .pa-hero-foot {
        font-size: 12px !important;
        color: var(--color-muted) !important;
        margin-top: 4px !important;
    }

    /* ── Generic state cards ── */
    .pa-state {
        margin-top: 24px;
        display: flex;
        gap: 14px;
        align-items: flex-start;
        padding: 20px;
        background: var(--color-panel);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card, 14px);
    }
    .pa-state :global(.pa-state-ico) {
        flex: none;
        width: 24px;
        height: 24px;
        color: var(--color-text-secondary);
    }
    .pa-state h2 {
        margin: 0 0 3px;
        font-size: 15px;
        font-weight: 600;
        color: var(--color-text);
    }
    .pa-state p {
        margin: 0;
        font-size: 13px;
        color: var(--color-text-secondary);
    }
    .pa-state-error {
        border-color: color-mix(in srgb, #ef4444 35%, var(--color-border));
    }
    .pa-state-error :global(.pa-state-ico) {
        color: #ef4444;
    }

    /* ── Score card ── */
    .pa-score-card {
        margin-top: 24px;
        display: flex;
        align-items: center;
        gap: 22px;
        padding: 22px 24px;
        background: var(--color-panel);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card, 14px);
        border-left-width: 4px;
    }
    .pa-tone-good {
        border-left-color: #22c55e;
    }
    .pa-tone-warn {
        border-left-color: #f59e0b;
    }
    .pa-tone-bad {
        border-left-color: #ef4444;
    }
    .pa-score-num {
        flex: none;
        display: flex;
        align-items: baseline;
        gap: 2px;
    }
    .pa-score-value {
        font-size: 44px;
        font-weight: 800;
        line-height: 1;
        color: var(--color-text);
    }
    .pa-tone-good .pa-score-value {
        color: #22c55e;
    }
    .pa-tone-warn .pa-score-value {
        color: #f59e0b;
    }
    .pa-tone-bad .pa-score-value {
        color: #ef4444;
    }
    .pa-score-max {
        font-size: 16px;
        font-weight: 600;
        color: var(--color-muted);
    }
    .pa-score-title {
        font-size: 15px;
        font-weight: 700;
        color: var(--color-text);
    }
    .pa-score-summary {
        margin-top: 2px;
        font-size: 13px;
        color: var(--color-text-secondary);
    }

    /* ── Active-now banner ── */
    .pa-active-banner {
        margin-top: 14px;
        display: flex;
        align-items: center;
        gap: 10px;
        padding: 12px 16px;
        font-size: 13px;
        color: var(--color-text);
        background: color-mix(in srgb, #ef4444 10%, transparent);
        border: 1px solid color-mix(in srgb, #ef4444 32%, var(--color-border));
        border-radius: var(--radius-control, 10px);
    }
    .pa-active-banner :global(.pa-active-ico) {
        flex: none;
        width: 18px;
        height: 18px;
        color: #ef4444;
    }

    /* ── Per-scan groups (collapsible accordion) ── */
    .pa-group {
        margin-top: 10px;
        background: var(--color-panel);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card, 12px);
        overflow: hidden;
    }
    .pa-group.is-open {
        border-color: color-mix(in srgb, var(--color-accent) 25%, var(--color-border));
    }
    .pa-group-head {
        display: flex;
        align-items: center;
        gap: 10px;
        width: 100%;
        padding: 12px 16px;
        background: transparent;
        border: none;
        text-align: left;
        cursor: pointer;
        color: var(--color-text);
        transition: background-color var(--dur-micro, 130ms) var(--ease-out, ease);
    }
    .pa-group-head:hover {
        background: var(--color-panel-2);
    }
    .pa-group-head :global(.pa-group-ico) {
        flex: none;
        width: 18px;
        height: 18px;
        color: var(--color-text-secondary);
    }
    .pa-group-head h3 {
        margin: 0;
        font-size: 14px;
        font-weight: 600;
        color: var(--color-text);
    }
    .pa-group-status {
        margin-left: auto;
        display: flex;
        align-items: center;
        gap: 10px;
    }
    .pa-status {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        font-size: 12px;
        font-weight: 600;
        white-space: nowrap;
    }
    .pa-status-clear {
        color: #22c55e;
    }
    .pa-status-clear :global(.pa-status-ico) {
        width: 14px;
        height: 14px;
    }
    .pa-status-muted {
        color: var(--color-muted);
        font-weight: 500;
    }
    /* Per-scan loading indicator — the card shows its own spinner the instant
       the scan kicks off, and stops the moment THAT scan resolves (independent
       of the slower scans still running). */
    .pa-status-loading {
        color: var(--color-text-secondary);
    }
    :global(.pa-status-spin) {
        width: 14px;
        height: 14px;
        color: var(--color-accent);
        animation: pa-spin 0.9s linear infinite;
    }
    @keyframes pa-spin {
        to {
            transform: rotate(360deg);
        }
    }
    .pa-status-flag {
        color: var(--color-text-secondary);
    }
    .pa-status-flag .pa-dot {
        margin-top: 0;
    }
    .pa-group-status :global(.pa-group-chev) {
        flex: none;
        width: 16px;
        height: 16px;
        color: var(--color-muted);
        transition: transform var(--dur-micro, 150ms) var(--ease-out, ease);
    }
    .pa-group-status :global(.pa-group-chev.is-open) {
        transform: rotate(180deg);
    }
    .pa-group-body {
        padding: 0 16px 14px;
    }
    .pa-group-summary {
        margin: 0 0 10px;
        font-size: 12.5px;
        color: var(--color-text-secondary);
    }
    .pa-soft-note {
        display: flex;
        gap: 12px;
        align-items: flex-start;
        padding: 14px 16px;
        background: var(--color-panel);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card, 12px);
    }
    .pa-soft-note :global(.pa-soft-ico) {
        flex: none;
        width: 18px;
        height: 18px;
        color: var(--color-text-secondary);
        margin-top: 1px;
    }
    .pa-soft-note p {
        margin: 0;
        font-size: 13px;
        color: var(--color-text-secondary);
    }
    .pa-clear-row {
        display: flex;
        align-items: center;
        gap: 8px;
        font-size: 13px;
        color: var(--color-text-secondary);
        padding: 6px 2px;
    }
    .pa-clear-row :global(.pa-clear-ico) {
        width: 16px;
        height: 16px;
        color: #22c55e;
    }
    .pa-list {
        display: flex;
        flex-direction: column;
        gap: 8px;
    }
    .pa-card {
        display: flex;
        align-items: flex-start;
        gap: 12px;
        padding: 12px 14px;
        background: var(--color-panel);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card, 12px);
    }
    .pa-dot {
        flex: none;
        width: 8px;
        height: 8px;
        margin-top: 5px;
        border-radius: 999px;
        background: var(--color-muted);
    }
    .pa-sev-high .pa-dot {
        background: #ef4444;
    }
    .pa-sev-medium .pa-dot {
        background: #f59e0b;
    }
    .pa-sev-low .pa-dot {
        background: #eab308;
    }
    .pa-sev-info .pa-dot {
        background: #3b82f6;
    }
    .pa-card-body {
        flex: 1;
        min-width: 0;
    }
    .pa-card-top {
        display: flex;
        align-items: center;
        gap: 8px;
    }
    .pa-card-title {
        font-size: 13.5px;
        font-weight: 600;
        color: var(--color-text);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .pa-tag {
        flex: none;
        font-size: 10px;
        font-weight: 700;
        text-transform: uppercase;
        letter-spacing: 0.04em;
        color: #ef4444;
        background: color-mix(in srgb, #ef4444 12%, transparent);
        border: 1px solid color-mix(in srgb, #ef4444 30%, var(--color-border));
        padding: 1px 7px;
        border-radius: 999px;
    }
    .pa-card-detail {
        margin-top: 3px;
        font-size: 12px;
        color: var(--color-text-secondary);
        word-break: break-word;
    }
    .pa-card-rec {
        margin-top: 5px;
        font-size: 12px;
        color: var(--color-muted);
    }

    /* Phase 6.5-5: per-finding kind chips + inline preview panel. */
    .pa-kinds {
        display: flex;
        flex-wrap: wrap;
        gap: 4px;
        margin-top: 4px;
        margin-bottom: 2px;
    }
    .pa-kind-chip {
        font-size: 10.5px;
        font-weight: 600;
        padding: 1px 7px;
        border-radius: 9999px;
        background: color-mix(in srgb, var(--color-accent) 14%, transparent);
        color: var(--color-accent);
        border: 1px solid color-mix(in srgb, var(--color-accent) 30%, transparent);
        letter-spacing: 0.2px;
    }
    .pa-preview {
        margin-top: 10px;
        padding: 10px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: 8px;
        font-size: 11.5px;
        max-height: 360px;
        overflow-y: auto;
    }
    .pa-preview-meta {
        font-size: 10.5px;
        text-transform: uppercase;
        letter-spacing: 0.5px;
        color: var(--color-text-secondary);
        margin-bottom: 6px;
    }
    /* One excerpt — a card-within-the-panel showing a few lines of
       context around each hit. Multiple excerpts stack with a small
       gap so the user can tell them apart. */
    .pa-excerpt {
        margin-top: 8px;
        background: var(--color-panel);
        border: 1px solid var(--color-border);
        border-radius: 6px;
        overflow: hidden;
    }
    .pa-excerpt:first-of-type {
        margin-top: 0;
    }
    .pa-excerpt-head {
        font-size: 10.5px;
        font-weight: 600;
        color: var(--color-text-secondary);
        background: var(--color-panel-2);
        padding: 3px 8px;
        border-bottom: 1px solid var(--color-border);
        text-transform: uppercase;
        letter-spacing: 0.4px;
    }
    .pa-excerpt-body {
        padding: 6px 8px;
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 11.5px;
        line-height: 1.5;
        white-space: pre-wrap;
        word-break: break-word;
    }
    .pa-preview-loading,
    .pa-preview-empty,
    .pa-preview-error,
    .pa-preview-trunc {
        font-family: var(--font-sans);
        font-size: 11.5px;
        padding: 4px 2px;
    }
    .pa-preview-loading {
        color: var(--color-text-secondary);
    }
    .pa-preview-empty {
        color: var(--color-text-secondary);
        font-style: italic;
    }
    .pa-preview-error {
        color: var(--color-error, #fb7185);
    }
    .pa-preview-trunc {
        color: var(--color-text-secondary);
        margin-top: 8px;
        border-top: 1px dashed var(--color-border);
        padding-top: 6px;
    }

    .pa-card-actions {
        flex: none;
        align-self: center;
        display: flex;
        flex-direction: column;
        align-items: stretch;
        gap: 6px;
    }
    .pa-fix {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        gap: 6px;
        padding: 6px 11px;
        font-size: 12px;
        font-weight: 600;
        color: var(--color-text);
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        cursor: pointer;
        white-space: nowrap;
        transition:
            background-color var(--dur-micro, 130ms) var(--ease-out, ease),
            border-color var(--dur-micro, 130ms) var(--ease-out, ease);
    }
    .pa-fix:hover {
        background: var(--color-panel);
        border-color: color-mix(in srgb, var(--color-accent) 45%, var(--color-border));
    }
    .pa-fix :global(.pa-fix-ico) {
        width: 13px;
        height: 13px;
    }
    /* Quiet "acknowledge / restore" action — visually subordinate to the fix. */
    .pa-ack {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        gap: 6px;
        padding: 6px 11px;
        font-size: 12px;
        font-weight: 600;
        color: var(--color-text-secondary);
        background: transparent;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        cursor: pointer;
        white-space: nowrap;
        transition:
            background-color var(--dur-micro, 130ms) var(--ease-out, ease),
            color var(--dur-micro, 130ms) var(--ease-out, ease);
    }
    .pa-ack:hover {
        background: var(--color-panel-2);
        color: var(--color-text);
    }
    .pa-ack :global(.pa-ack-ico) {
        width: 13px;
        height: 13px;
    }

    /* ── Acknowledged (collapsed) ── */
    .pa-acked {
        margin-top: 28px;
        border-top: 1px solid var(--color-border);
        padding-top: 14px;
    }
    .pa-acked-toggle {
        display: flex;
        align-items: center;
        gap: 8px;
        width: 100%;
        padding: 6px 2px;
        background: transparent;
        border: none;
        color: var(--color-text-secondary);
        font-size: 13px;
        font-weight: 600;
        cursor: pointer;
    }
    .pa-acked-toggle:hover {
        color: var(--color-text);
    }
    .pa-acked-toggle :global(.pa-acked-ico) {
        width: 15px;
        height: 15px;
        color: #22c55e;
    }
    .pa-acked-toggle :global(.pa-acked-chev) {
        width: 15px;
        height: 15px;
        margin-left: auto;
        transition: transform var(--dur-micro, 150ms) var(--ease-out, ease);
    }
    .pa-acked-toggle :global(.pa-acked-chev.is-open) {
        transform: rotate(180deg);
    }
    .pa-acked-list {
        margin-top: 10px;
    }
    .pa-acked-card {
        align-items: center;
        opacity: 0.72;
    }
</style>
