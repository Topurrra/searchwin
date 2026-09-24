<script lang="ts">
    /*
      Secret Leak Scanner — Phase 6.5-4 (2026-05-27) rewrite.

      Previous version ran ~150 lines of JS regex rules locally. Those
      rules duplicated (and inferior-quality'd) the backend's
      sensitive_scan.rs — e.g. the AWS-secret-key rule matched any
      40-char base64 string, an enormous false-positive surface.
      Phase 6.5-1 promoted the backend to span + tier output;
      Phase 6.5-4 routes this tool through it.

      What changed for the user:
        • Far fewer false positives. The backend rejects placeholders
          (`YOUR_API_KEY_HERE`), env refs (`$DB_PASS`, `process.env.X`),
          and `Bearer` tokens that don't carry enough entropy.
        • Three real tiers (HIGH / MEDIUM / LOW) replace the JS
          severity guesses. Presets now map directly to tiers.
        • PrivacyBlur on every matched value — review without leaking
          to a bystander or to a screen-share viewer.
        • "Mark as not sensitive" (Phase 6.5-2 allowlist) — one click
          dismisses a false positive forever; the same string is never
          re-flagged anywhere in the app.
    */
    import { invoke } from '@tauri-apps/api/core';
    import {
        AlertCircle,
        Copy,
        Search,
        ShieldCheck,
        ShieldAlert,
        XCircle,
    } from '@lucide/svelte';
    import { toast } from '$lib/stores/toasts';
    import { errorToast } from '$lib/stores/errorToast';
    import { Button, Checkbox, ToolPanel, EmptyState } from '$lib/ui';
    import PrivacyBlur from '$lib/components/PrivacyBlur.svelte';
    import {
        type SensitiveFinding,
        type SensitiveTier,
        sensitiveKindLabel,
    } from '$lib/types/sensitive';
    // State lifted into a module store so a typed snippet + scan results survive
    // leaving and returning to Developer Tools (panels remount fresh on nav).
    import {
        secretScanInput,
        secretScanPreset,
        secretScanAutoRedact,
        secretScanFindings,
        secretScanProcessing,
        secretScanError,
    } from '$lib/stores/devkitPanels';

    /** A tool preset bundles "which tiers do we scan?" + UX defaults
     *  (auto-redact, description). Three presets cover the common
     *  intents — finding leaks, finding personal data, pre-release check. */
    interface ScanPreset {
        id: 'security' | 'privacy' | 'release';
        label: string;
        tiers: SensitiveTier[];
        /** True when the LOW-tier toggle is included in the backend
         *  call. Only the Privacy + Release presets need it; Security
         *  ignores low-tier shapes (email/phone/IP aren't secrets). */
        includeLow: boolean;
        autoRedact: boolean;
        description: string;
    }

    const presets: ScanPreset[] = [
        {
            id: 'security',
            label: 'Security',
            tiers: ['high', 'medium'],
            includeLow: false,
            autoRedact: true,
            description:
                'Tokens, keys, and credentials. The default — secrets before they ship.',
        },
        {
            id: 'privacy',
            label: 'Privacy',
            tiers: ['low'],
            includeLow: true,
            autoRedact: true,
            description:
                'Personal data: email, phone, IP. Off-by-default elsewhere — opt in here.',
        },
        {
            id: 'release',
            label: 'Release check',
            tiers: ['high', 'medium', 'low'],
            includeLow: true,
            autoRedact: false,
            description: 'Combined scan before packaging code or docs.',
        },
    ];

    let isScanning = $derived($secretScanProcessing);
    let error = $derived($secretScanError);

    // Typed view of the loosely-typed findings store so the derived
    // computations below keep their SensitiveFinding shape.
    const findings = $derived($secretScanFindings as SensitiveFinding[]);

    const selectedPreset = $derived(
        presets.find((preset) => preset.id === $secretScanPreset) ?? presets[0],
    );

    /** Findings filtered to the tiers the active preset cares about.
     *  The backend already filters via include_low, but tier filtering
     *  here lets a Release-scan view be re-pivoted to "show only HIGH"
     *  without a re-scan. */
    const visibleFindings = $derived(
        findings.filter((f) => selectedPreset.tiers.includes(f.tier)),
    );

    /** Risk score 0-100 — tier-weighted finding count, capped. The
     *  weights mirror the spirit of the old severity table but tied
     *  to real backend tiers instead of frontend guesses. */
    const riskScore = $derived.by(() => {
        const weight: Record<SensitiveTier, number> = {
            high: 45,
            medium: 15,
            low: 3,
        };
        let score = 0;
        for (const f of visibleFindings) {
            score += weight[f.tier];
        }
        return Math.max(0, Math.min(100, score));
    });

    /** Sanitized version of `input` — every visible finding's matched
     *  text replaced with `[KIND]`. Built by walking findings sorted
     *  end → start so earlier replacements don't shift later offsets. */
    const sanitized = $derived.by(() => {
        if (visibleFindings.length === 0) return $secretScanInput;
        const sorted = [...visibleFindings].sort((a, b) => b.start - a.start);
        let result = $secretScanInput;
        for (const f of sorted) {
            const marker = `[${f.kind.toUpperCase()}]`;
            result = result.slice(0, f.start) + marker + result.slice(f.end);
        }
        return result;
    });

    /** Findings grouped by kind for the results panel — easier to scan
     *  than a flat list when one file has 10× the same token. */
    interface FindingGroup {
        kind: string;
        label: string;
        tier: SensitiveTier;
        items: SensitiveFinding[];
    }
    const groupedFindings = $derived.by<FindingGroup[]>(() => {
        const byKind = new Map<string, FindingGroup>();
        for (const f of visibleFindings) {
            const g = byKind.get(f.kind);
            if (g) {
                g.items.push(f);
            } else {
                byKind.set(f.kind, {
                    kind: f.kind,
                    label: sensitiveKindLabel(f.kind),
                    tier: f.tier,
                    items: [f],
                });
            }
        }
        // Highest-tier groups first (HIGH before MEDIUM before LOW),
        // then by item count descending — the noisy categories first.
        const tierOrder: Record<SensitiveTier, number> = {
            high: 0,
            medium: 1,
            low: 2,
        };
        return [...byKind.values()].sort(
            (a, b) =>
                tierOrder[a.tier] - tierOrder[b.tier] ||
                b.items.length - a.items.length,
        );
    });

    /** Slice the matched text out of `input` for a finding. Defensive
     *  bounds — a malformed span returns the raw kind label so the UI
     *  never crashes on weird input. */
    function findingText(f: SensitiveFinding): string {
        if (f.end > $secretScanInput.length || f.start >= f.end) return '';
        return $secretScanInput.slice(f.start, f.end);
    }

    /** Tier → color class for the chip in the findings list. */
    function tierChipClass(tier: SensitiveTier) {
        if (tier === 'high') return 'is-high';
        if (tier === 'medium') return 'is-medium';
        return 'is-low';
    }

    async function runScan() {
        if (isScanning) return;
        if (!$secretScanInput.trim()) {
            $secretScanFindings = [];
            $secretScanError = null;
            return;
        }
        $secretScanProcessing = true;
        $secretScanError = null;
        try {
            const result = await invoke<SensitiveFinding[]>('scan_text_for_findings', {
                text: $secretScanInput,
                includeLow: selectedPreset.includeLow,
            });
            $secretScanFindings = result;
        } catch (e) {
            $secretScanError = `Scan failed: ${e}`;
            $secretScanFindings = [];
        } finally {
            $secretScanProcessing = false;
        }
    }

    /** Dismiss a single finding via the Phase 6.5-2 allowlist. The
     *  string is hashed with the device salt before being stored —
     *  raw secret never lands on disk. After dismissal we re-run the
     *  scan so the UI immediately reflects the new state. */
    async function dismissFinding(f: SensitiveFinding) {
        const text = findingText(f);
        if (!text) return;
        try {
            await invoke('dismiss_finding', { text });
            toast(`Dismissed "${sensitiveKindLabel(f.kind)}". Won't be flagged again.`, 'success', 3000);
            await runScan();
        } catch (e) {
            errorToast("Couldn't add this finding to your allowlist", e, {
                hint: 'The allowlist file may be locked by another scan — wait a moment and try again.',
            });
        }
    }

    function clearAll() {
        $secretScanInput = '';
        $secretScanFindings = [];
        $secretScanError = null;
    }

    async function copyText(value: string, label: string) {
        if (!value) return;
        try {
            await navigator.clipboard.writeText(value);
            toast(`Copied ${label}`, 'success');
        } catch {
            toast('Clipboard copy failed', 'error');
        }
    }

    function setPreset(id: ScanPreset['id']) {
        $secretScanPreset = id;
        const preset = presets.find((p) => p.id === id);
        if (preset) {
            $secretScanAutoRedact = preset.autoRedact;
        }
    }

    function loadDemo() {
        $secretScanInput = `# Example CI secrets snippet
OPENAI_API_KEY=sk-1234567890abcdefghijklmnopqrstuvw
DATABASE_URL=postgres://admin:Pa$$w0rd123!@localhost:5432/app
GITHUB_TOKEN=ghp_1234567890abcdefghijklmnopqrstuvwxyzAB
SSN=123-45-6789
user.email=john.doe@example.com
Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U`;
        void runScan();
    }

    async function restoreAllowlist() {
        try {
            const removed = await invoke<number>('restore_allowlist');
            toast(
                removed === 0
                    ? 'Allowlist was already empty.'
                    : `Restored ${removed} dismissed finding${removed === 1 ? '' : 's'}.`,
                'success',
                3500,
            );
            await runScan();
        } catch (e) {
            toast(`Restore failed: ${e}`, 'error');
        }
    }
</script>

<div class="dt-panel">
    <div class="dt-head">
        <div class="dt-head-text">
            <h2 class="dt-title">Secret Scanner — find leaked credentials &amp; PII</h2>
            <p class="dt-desc">Paste a snippet, config, or log and scan it for credentials, tokens, and PII before sharing or committing. All scanning runs on-device.</p>
        </div>
    </div>

    <!-- Toolbar -->
    <div class="dt-toolbar">
        <Button variant="secondary" onclick={loadDemo}>Sample</Button>
        <Button variant="ghost" onclick={clearAll}>Clear</Button>
        <Button
            variant="ghost"
            onclick={() => void restoreAllowlist()}
            aria-label="Bring back every dismissed finding"
        >Restore allowlist</Button>
        <div class="dt-toolbar-end">
            <Button
                variant="primary"
                icon={Search}
                loading={isScanning}
                disabled={isScanning}
                onclick={() => void runScan()}
            >{isScanning ? 'Scanning…' : 'Scan'}</Button>
        </div>
    </div>

    <!-- Preset picker -->
    <div class="dt-io-head">
        <span class="dt-section-label">Preset</span>
    </div>
    <div class="dt-toolbar">
        <div class="dt-seg" role="group" aria-label="Preset">
            {#each presets as preset}
                <button class="dt-seg-btn" class:is-active={$secretScanPreset === preset.id} onclick={() => setPreset(preset.id)}>
                    {preset.label}
                </button>
            {/each}
        </div>
        <span class="dt-hint ss-preset-note">{selectedPreset.description}</span>
    </div>

    <div class="dt-two">
        <!-- Source input -->
        <div class="dt-io">
            <div class="dt-io-head">
                <span class="dt-section-label">Paste to scan</span>
                <span class="dt-hint">{$secretScanInput.length} chars</span>
            </div>
            <textarea
                id="leak-input"
                class="dt-ta is-tall"
                bind:value={$secretScanInput}
                spellcheck="false"
                placeholder="Paste JSON, .env, logs, markdown, SQL, or config here..."
            ></textarea>
            <div class="ss-input-foot">
                <Checkbox bind:checked={$secretScanAutoRedact} label="Auto redact output" />
            </div>
        </div>

        <!-- Scan summary -->
        <ToolPanel padding="md">
            <div class="dt-io-head">
                <span class="dt-section-label">Scan summary</span>
            </div>
            <div class="dt-stats cols-3">
                <div class="dt-stat">
                    <div class="dt-stat-label">Findings</div>
                    <div class="dt-stat-val {visibleFindings.length > 0 ? 'is-error' : ''}">{visibleFindings.length}</div>
                </div>
                <div class="dt-stat">
                    <div class="dt-stat-label">Risk score</div>
                    <div class="dt-stat-val {riskScore > 0 ? 'is-error' : ''}">{riskScore}/100</div>
                </div>
                <div class="dt-stat">
                    <div class="dt-stat-label">Tiers</div>
                    <div class="dt-stat-val is-accent">{selectedPreset.tiers.length}</div>
                </div>
            </div>

            <div class="ss-risk-track" aria-hidden="true">
                <div class="ss-risk-fill" style={`width:${riskScore}%`}></div>
            </div>

            <div class="ss-summary-actions">
                <Button
                    variant="secondary"
                    icon={Copy}
                    onclick={() => void copyText($secretScanAutoRedact ? sanitized : $secretScanInput, 'output')}
                >Copy {$secretScanAutoRedact ? 'sanitized' : 'raw'}</Button>
                <Button
                    variant="ghost"
                    icon={Copy}
                    onclick={() => void copyText($secretScanInput, 'raw input')}
                >Copy raw</Button>
            </div>

            <div class="dt-io-head ss-tiers-head">
                <span class="dt-section-label">Tiers enabled</span>
            </div>
            <div class="ss-tiers">
                {#each selectedPreset.tiers as tier}
                    <div class="dt-kv">
                        <span class="dt-kv-key ss-tier-name">{tier}</span>
                        <ShieldCheck class="ss-tier-ico dt-text-success" />
                    </div>
                {/each}
            </div>
        </ToolPanel>
    </div>

    <!-- Findings list -->
    {#if visibleFindings.length > 0}
        <ToolPanel padding="md">
            <div class="dt-io-head">
                <span class="dt-section-label">Findings</span>
            </div>
            <div class="dt-results ss-results">
                {#each groupedFindings as group (group.kind)}
                    <div class="ss-group">
                        <div class="dt-result-row">
                            <span class="ss-tier-chip {tierChipClass(group.tier)}">{group.tier}</span>
                            <span class="dt-result-name">{group.label}</span>
                            <span class="dt-hint ss-occurrences">
                                {group.items.length} occurrence{group.items.length === 1 ? '' : 's'}
                            </span>
                        </div>
                        <div class="ss-group-items">
                            {#each group.items as item, idx (item.start + ':' + item.end + ':' + idx)}
                                {@const matched = findingText(item)}
                                <div class="dt-result ss-finding-row">
                                    <div class="dt-result-detail dt-mono ss-finding-blur">
                                        <PrivacyBlur
                                            text={matched}
                                            findings={[{ ...item, start: 0, end: matched.length }]}
                                            blurTiers={['high', 'medium', 'low']}
                                        />
                                    </div>
                                    <button
                                        type="button"
                                        class="ss-dismiss"
                                        onclick={() => void dismissFinding(item)}
                                        aria-label="Mark this exact value as not sensitive (won't be flagged again)"
                                        title="Mark this exact value as 'not sensitive' (won't be flagged again)"
                                    >
                                        <XCircle class="ss-dismiss-ico" />
                                        Not sensitive
                                    </button>
                                </div>
                            {/each}
                        </div>
                    </div>
                {/each}
            </div>
        </ToolPanel>
    {:else if !isScanning}
        <ToolPanel padding="md">
            <EmptyState
                icon={ShieldCheck}
                title={$secretScanInput.trim() ? 'No secrets found' : 'Paste something to scan'}
                description={$secretScanInput.trim()
                    ? 'No findings for the active preset. Try Release check for a broader sweep.'
                    : 'Drop in a config, .env, or log to inspect.'}
                variant="compact"
            />
        </ToolPanel>
    {/if}

    <!-- Sanitized preview -->
    <div class="dt-io">
        <div class="dt-io-head">
            <span class="dt-section-label">
                <ShieldAlert class="ss-head-ico dt-text-accent" /> Sanitized preview
            </span>
        </div>
        <textarea
            class="dt-ta is-tall ss-preview"
            value={$secretScanAutoRedact ? sanitized : $secretScanInput}
            readonly
            spellcheck="false"
        ></textarea>
    </div>

    {#if error}
        <div class="dt-error">{error}</div>
    {/if}

    <div class="dt-note">
        <AlertCircle class="ss-note-ico" />
        {visibleFindings.length === 0
            ? 'No confirmed patterns yet.'
            : 'Click a blurred value to reveal it. Ctrl+Shift+R reveals all. Use Not sensitive to dismiss a false positive.'}
    </div>
</div>

<style>
    /* Local-only layout + the tier chip language. dt-* classes stay
       global (owned by the parent shell) and are never redefined here. */

    /* Preset note pushes to the trailing edge of the toolbar row. */
    .ss-preset-note {
        margin-left: auto;
    }

    /* Auto-redact toggle below the input. */
    .ss-input-foot {
        margin-top: 8px;
    }

    /* Risk meter: track + accent fill, colors from theme tokens. */
    .ss-risk-track {
        height: 8px;
        margin-top: 10px;
        border-radius: 9999px;
        border: 1px solid var(--color-border);
        background: var(--color-panel);
        overflow: hidden;
    }
    .ss-risk-fill {
        height: 100%;
        background: var(--color-accent);
    }

    .ss-summary-actions {
        display: flex;
        flex-wrap: wrap;
        gap: 8px;
        margin-top: 12px;
    }

    .ss-tiers-head {
        margin-top: 14px;
    }
    .ss-tiers {
        display: grid;
        gap: 4px;
        margin-top: 6px;
    }
    .ss-tier-name {
        text-transform: capitalize;
    }
    .ss-tiers :global(.ss-tier-ico) {
        width: 14px;
        height: 14px;
    }

    /* Findings groups. */
    .ss-results {
        max-height: 60vh;
        overflow-y: auto;
    }
    .ss-group {
        display: flex;
        flex-direction: column;
        gap: 8px;
    }
    .ss-occurrences {
        margin-left: auto;
    }
    .ss-group-items {
        display: flex;
        flex-direction: column;
        gap: 8px;
    }

    /* Tier chip: colors track the rest of the app's severity language
       (high = error/red, medium = warning/amber, low = accent/blue),
       sourced from theme tokens rather than hardcoded hex. */
    .ss-tier-chip {
        display: inline-flex;
        align-items: center;
        padding: 1px 8px;
        border-radius: 9999px;
        font-size: 10.5px;
        text-transform: uppercase;
        letter-spacing: 0.5px;
        font-weight: 600;
        border: 1px solid;
        flex-shrink: 0;
    }
    .ss-tier-chip.is-high {
        background: color-mix(in srgb, var(--color-error) 14%, transparent);
        color: var(--color-error);
        border-color: color-mix(in srgb, var(--color-error) 40%, transparent);
    }
    .ss-tier-chip.is-medium {
        background: color-mix(in srgb, var(--color-warning) 14%, transparent);
        color: var(--color-warning);
        border-color: color-mix(in srgb, var(--color-warning) 40%, transparent);
    }
    .ss-tier-chip.is-low {
        background: color-mix(in srgb, var(--color-accent) 14%, transparent);
        color: var(--color-accent);
        border-color: color-mix(in srgb, var(--color-accent) 40%, transparent);
    }

    /* One finding row: blur on the left, dismiss button on the right. */
    .ss-finding-row {
        display: flex;
        gap: 8px;
        align-items: flex-start;
    }
    .ss-finding-blur {
        flex: 1;
        min-width: 0;
        font-size: 11.5px;
        overflow-x: auto;
        word-break: break-all;
    }
    .ss-dismiss {
        appearance: none;
        border: 1px solid var(--color-border);
        background: transparent;
        color: var(--color-text-secondary);
        padding: 3px 8px;
        font-size: 11px;
        border-radius: 6px;
        cursor: pointer;
        display: inline-flex;
        align-items: center;
        gap: 4px;
        flex-shrink: 0;
        transition: background 120ms ease, color 120ms ease;
    }
    .ss-dismiss:hover {
        background: var(--color-panel-3);
        color: var(--color-text);
    }
    .ss-dismiss :global(.ss-dismiss-ico) {
        width: 14px;
        height: 14px;
    }

    /* Sanitized preview reads as mono and wraps long tokens. */
    .ss-preview {
        word-break: break-all;
    }

    /* Inline icons in section labels / disclaimer. */
    .dt-section-label :global(.ss-head-ico) {
        width: 14px;
        height: 14px;
        vertical-align: -2px;
    }
    .dt-note :global(.ss-note-ico) {
        width: 14px;
        height: 14px;
        flex-shrink: 0;
    }
</style>
