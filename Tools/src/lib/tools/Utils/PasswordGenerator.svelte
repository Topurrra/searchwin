<script lang="ts">
    import { copySecret } from '$lib/utils/secretCopy';
    import { Copy, Eye, EyeOff, RefreshCw, ShieldCheck, KeyRound } from '@lucide/svelte';
    import { toast } from '$lib/stores/toasts';
    import { _ } from 'svelte-i18n';
    import { escToClear } from '$lib/actions/escToClear';
    import { ToolPage } from '$lib/ui';
    import { EFF_LARGE_WORDLIST } from '$lib/data/effLargeWordlist';

    type PasswordMode = 'random' | 'passphrase' | 'secret';
    type SecretFormat = 'raw' | 'hex' | 'base32' | 'base64' | 'base64url';

    let mode = $state<PasswordMode>('random');

    let length = $state(20);
    let useUpper = $state(true);
    let useLower = $state(true);
    let useDigits = $state(true);
    let useSymbols = $state(true);
    let excludeAmbiguous = $state(false);
    let customSymbols = $state('!@#$%^&*()-_=+[]{}|;:,.<>?');
    let count = $state(10);

    let wordCount = $state(4);
    let wordSeparator = $state('-');
    let capitalizeWords = $state(true);
    let appendNumber = $state(true);

    let showPasswords = $state(false);
    let passwords = $state<string[]>([]);

    let secretOutputCount = $state(8);
    let secretFormat = $state<SecretFormat>('base64url');
    let secretBytes = $state(32);
    let addSecretPrefix = $state(true);
    let secretPrefix = $state('sk_live_');
    let secretOutputs = $state<string[]>([]);
    let secretAuditInput = $state('');

    // The EFF "large" diceware list — 7776 words, ~12.925 bits of entropy per word.
    // (Replaces a 64-word list that gave only ~6 bits/word yet was labelled "Strong".)
    const wordList = EFF_LARGE_WORDLIST;

    const base32Alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567';
    const rawSecretAlphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789';

    function randomInt(min: number, max: number): number {
        const arr = new Uint32Array(1);
        crypto.getRandomValues(arr);
        return min + (arr[0] % (max - min + 1));
    }

    function randomBytes(lengthBytes: number): Uint8Array {
        const out = new Uint8Array(Math.max(1, Math.floor(lengthBytes)));
        crypto.getRandomValues(out);
        return out;
    }

    function bytesToBinary(bytes: Uint8Array): string {
        let out = '';
        for (const value of bytes) {
            out += String.fromCharCode(value);
        }
        return out;
    }

    function toBase32(bytes: Uint8Array): string {
        const bits = Array.from(bytes).map((b) => b.toString(2).padStart(8, '0')).join('');
        const chunks = bits.match(/.{1,5}/g) ?? [];
        const end = chunks[chunks.length - 1];
        const symbols = chunks.map((chunk, idx) => {
            if (end !== undefined && idx === chunks.length - 1 && end.length < 5) {
                return base32Alphabet[parseInt(chunk.padEnd(5, '0'), 2)];
            }
            return base32Alphabet[parseInt(chunk, 2)];
        });
        return symbols.join('');
    }

    function encodeSecretBytes(bytes: Uint8Array): string {
        const raw = bytesToBinary(bytes);
        if (secretFormat === 'base64') {
            return btoa(raw);
        }
        if (secretFormat === 'base64url') {
            return btoa(raw).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/g, '');
        }
        if (secretFormat === 'hex') {
            return Array.from(bytes).map((b) => b.toString(16).padStart(2, '0')).join('');
        }
        if (secretFormat === 'base32') {
            return toBase32(bytes);
        }
        return Array.from(bytes).map((b) => rawSecretAlphabet[b % rawSecretAlphabet.length]).join('');
    }

    function pickFrom(arr: string | string[]): string {
        const idx = randomInt(0, arr.length - 1);
        return arr[idx];
    }

    function generateRandom(): string {
        let chars = '';
        const upper = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ';
        const lower = 'abcdefghijklmnopqrstuvwxyz';
        const digits = '0123456789';
        const ambiguous = '0O1lI';

        if (useUpper) chars += upper;
        if (useLower) chars += lower;
        if (useDigits) chars += digits;
        if (useSymbols) chars += customSymbols;

        if (excludeAmbiguous) chars = chars.split('').filter((c) => !ambiguous.includes(c)).join('');
        if (!chars) chars = lower + digits;

        const required: string[] = [];
        let filteredUpper = upper;
        let filteredLower = lower;
        let filteredDigits = digits;
        if (excludeAmbiguous) {
            filteredUpper = upper.split('').filter((c) => !ambiguous.includes(c)).join('');
            filteredLower = lower.split('').filter((c) => !ambiguous.includes(c)).join('');
            filteredDigits = digits.split('').filter((c) => !ambiguous.includes(c)).join('');
        }
        if (useUpper && filteredUpper) required.push(pickFrom(filteredUpper));
        if (useLower && filteredLower) required.push(pickFrom(filteredLower));
        if (useDigits && filteredDigits) required.push(pickFrom(filteredDigits));
        if (useSymbols && customSymbols) required.push(pickFrom(customSymbols));

        const remaining = length - required.length;
        const rest = Array.from({ length: Math.max(0, remaining) }, () => pickFrom(chars));
        const combined = [...required, ...rest];
        for (let i = combined.length - 1; i > 0; i--) {
            const j = randomInt(0, i);
            [combined[i], combined[j]] = [combined[j], combined[i]];
        }
        return combined.join('');
    }

    function generatePassphrase(): string {
        const words = Array.from({ length: wordCount }, () => {
            const w = pickFrom(wordList);
            return capitalizeWords ? w.charAt(0).toUpperCase() + w.slice(1) : w;
        });
        let phrase = words.join(wordSeparator);
        if (appendNumber) phrase += `${wordSeparator}${randomInt(10, 9999)}`;
        return phrase;
    }

    function generateSecretValues(): string[] {
        return Array.from({ length: secretOutputCount }, () => {
            const encoded = encodeSecretBytes(randomBytes(secretBytes));
            return addSecretPrefix ? `${secretPrefix}${encoded}` : encoded;
        });
    }

    function entropy(password: string): number {
        const charset = new Set(password).size;
        return Math.round(password.length * Math.log2(Math.max(charset, 2)));
    }

    /** Passphrase strength is the SELECTION space, not the output characters: each
     *  word is one of `EFF_LARGE_WORDLIST` (7776 → ~12.925 bits) and the optional
     *  appended number is `randomInt(10, 9999)` → 9990 values. Capitalization and
     *  the separator are deterministic, so they add no entropy. */
    function passphraseEntropy(): number {
        const wordBits = wordCount * Math.log2(EFF_LARGE_WORDLIST.length);
        const numBits = appendNumber ? Math.log2(9990) : 0;
        return Math.round(wordBits + numBits);
    }

    function strengthLabel(e: number): { label: string; color: string } {
        if (e < 28) return { label: 'tool.passwordGenerator.strengthVeryWeak', color: 'text-error' };
        if (e < 36) return { label: 'tool.passwordGenerator.strengthWeak', color: 'text-warning' };
        if (e < 60) return { label: 'tool.passwordGenerator.strengthReasonable', color: 'text-warning' };
        if (e < 96) return { label: 'tool.passwordGenerator.strengthStrong', color: 'text-accent' };
        return { label: 'tool.passwordGenerator.strengthVeryStrong', color: 'text-success' };
    }

    /** Secret-audit issues — each entry is an i18n key plus optional
     * interpolation values, resolved with $_() at render time. */
    function secretRisk(input: string): { key: string; values?: Record<string, string> }[] {
        const lowered = input.toLowerCase();
        const issues: { key: string; values?: Record<string, string> }[] = [];
        if (input.length < 20) issues.push({ key: 'tool.passwordGenerator.riskTooShort' });
        if (!/[A-Z]/.test(input)) issues.push({ key: 'tool.passwordGenerator.riskNoUppercase' });
        if (!/[a-z]/.test(input)) issues.push({ key: 'tool.passwordGenerator.riskNoLowercase' });
        if (!/[0-9]/.test(input)) issues.push({ key: 'tool.passwordGenerator.riskNoDigits' });
        if (!/[^A-Za-z0-9]/.test(input)) issues.push({ key: 'tool.passwordGenerator.riskNoSymbols' });
        if (/(.)\1{3,}/.test(input)) issues.push({ key: 'tool.passwordGenerator.riskRepeatedRuns' });
        if (/(.)\\1/.test(input)) issues.push({ key: 'tool.passwordGenerator.riskAdjacentDuplicates' });
        const common = ['password', 'admin', 'secret', 'tok', 'api', 'key', 'example', '123456', 'qwerty', 'letmein'];
        for (const c of common) {
            if (lowered.includes(c)) {
                issues.push({ key: 'tool.passwordGenerator.riskPredictableFragment', values: { fragment: c } });
                break;
            }
        }
        return issues;
    }

    function runGenerator() {
        if (mode === 'random') {
            passwords = Array.from({ length: count }, () => generateRandom());
        } else if (mode === 'passphrase') {
            passwords = Array.from({ length: count }, () => generatePassphrase());
        } else {
            secretOutputs = generateSecretValues();
            passwords = [];
        }
    }

    function copyAll() {
        const list = mode === 'secret' ? secretOutputs : passwords;
        void copySecret(list.join('\n'));
        toast($_('tool.passwordGenerator.copiedValues', { values: { count: list.length } }), 'success');
    }

    async function copyOne(value: string, label: string) {
        await copySecret(value);
        toast($_('tool.passwordGenerator.copiedLabel', { values: { label } }), 'success');
    }

    $effect(() => {
        mode;
        length;
        useUpper;
        useLower;
        useDigits;
        useSymbols;
        excludeAmbiguous;
        customSymbols;
        count;
        wordCount;
        wordSeparator;
        capitalizeWords;
        appendNumber;
        secretOutputCount;
        secretBytes;
        secretFormat;
        addSecretPrefix;
        secretPrefix;
        runGenerator();
    });
</script>

<ToolPage
    icon={KeyRound}
    iconTint="#f59e0b"
    title={$_('tool.passwordGenerator.heroTitle')}
    description={$_('tool.passwordGenerator.heroDescription')}
    width="wide"
    fill={false}
>

    <div class="pg-workspace">
    <!-- Mode tabs -->
    <div class="pg-config rounded-2xl border border-border bg-panel p-3 md:p-4">
        <div class="pg-mode-tabs flex flex-wrap gap-2 border-b border-border pb-1">
            <button type="button" class="pg-mode-tab" class:is-active={mode === 'random'} aria-pressed={mode === 'random'} onclick={() => (mode = 'random')}>{$_('tool.passwordGenerator.modeRandom')}</button>
            <button type="button" class="pg-mode-tab" class:is-active={mode === 'passphrase'} aria-pressed={mode === 'passphrase'} onclick={() => (mode = 'passphrase')}>{$_('tool.passwordGenerator.modePassphrase')}</button>
            <button type="button" class="pg-mode-tab" class:is-active={mode === 'secret'} aria-pressed={mode === 'secret'} onclick={() => (mode = 'secret')}>{$_('tool.passwordGenerator.modeSecret')}</button>
        </div>

        <div class="pt-4 space-y-4">
            {#if mode === 'random'}
                <div>
                    <label for="pw-length" class="block text-xs uppercase tracking-wider text-muted font-semibold mb-2">{$_('tool.passwordGenerator.length', { values: { count: length } })}</label>
                    <input id="pw-length" type="range" min="8" max="128" bind:value={length} class="w-full accent-emerald-500" />
                    <div class="flex justify-between text-[10px] text-muted mt-0.5"><span>8</span><span>32</span><span>64</span><span>128</span></div>
                </div>

                <div class="grid gap-2 sm:grid-cols-2">
                    <label class="pg-option flex items-center gap-2 text-sm cursor-pointer bg-panel-2 border border-border rounded-lg p-2">
                        <input type="checkbox" bind:checked={useUpper} class="accent-emerald-500" />
                        <span>{$_('tool.passwordGenerator.uppercase')}</span>
                    </label>
                    <label class="pg-option flex items-center gap-2 text-sm cursor-pointer bg-panel-2 border border-border rounded-lg p-2">
                        <input type="checkbox" bind:checked={useLower} class="accent-emerald-500" />
                        <span>{$_('tool.passwordGenerator.lowercase')}</span>
                    </label>
                    <label class="pg-option flex items-center gap-2 text-sm cursor-pointer bg-panel-2 border border-border rounded-lg p-2">
                        <input type="checkbox" bind:checked={useDigits} class="accent-emerald-500" />
                        <span>{$_('tool.passwordGenerator.digits')}</span>
                    </label>
                    <label class="pg-option flex items-center gap-2 text-sm cursor-pointer bg-panel-2 border border-border rounded-lg p-2">
                        <input type="checkbox" bind:checked={useSymbols} class="accent-emerald-500" />
                        <span>{$_('tool.passwordGenerator.symbols')}</span>
                    </label>
                </div>

                {#if useSymbols}
                    <div>
                        <label for="symbols" class="block text-xs uppercase tracking-wider text-muted font-semibold mb-2">{$_('tool.passwordGenerator.symbolSet')}</label>
                        <input id="symbols" type="text" bind:value={customSymbols} use:escToClear={() => (customSymbols = '')} class="w-full px-3 py-2 bg-panel-2 border border-border rounded-lg font-mono text-sm focus:outline-none focus:border-accent transition-colors" />
                    </div>
                {/if}

                <label class="pg-option pg-option-wide flex items-center gap-2 text-sm cursor-pointer">
                    <input type="checkbox" bind:checked={excludeAmbiguous} class="accent-emerald-500" />
                    <span>{$_('tool.passwordGenerator.excludeAmbiguous')} <span class="text-muted font-mono">{$_('tool.passwordGenerator.excludeAmbiguousHint')}</span></span>
                </label>
            {/if}

            {#if mode === 'passphrase'}
                <div>
                    <label for="pp-words" class="block text-xs uppercase tracking-wider text-muted font-semibold mb-2">{$_('tool.passwordGenerator.words', { values: { count: wordCount } })}</label>
                    <input id="pp-words" type="range" min="3" max="8" bind:value={wordCount} class="w-full accent-emerald-500" />
                </div>

                <div class="grid gap-4 sm:grid-cols-2">
                    <div>
                        <label for="sep" class="block text-xs uppercase tracking-wider text-muted font-semibold mb-2">{$_('tool.passwordGenerator.separator')}</label>
                        <input id="sep" type="text" bind:value={wordSeparator} use:escToClear={() => (wordSeparator = '')} maxlength="5" class="w-full px-3 py-2 bg-panel-2 border border-border rounded-lg font-mono text-sm focus:outline-none focus:border-accent transition-colors" />
                    </div>
                </div>

                <div class="space-y-2">
                    <label class="pg-option pg-option-wide flex items-center gap-2 text-sm cursor-pointer">
                        <input type="checkbox" bind:checked={capitalizeWords} class="accent-emerald-500" />
                        <span>{$_('tool.passwordGenerator.capitalizeWords')}</span>
                    </label>
                    <label class="pg-option pg-option-wide flex items-center gap-2 text-sm cursor-pointer">
                        <input type="checkbox" bind:checked={appendNumber} class="accent-emerald-500" />
                        <span>{$_('tool.passwordGenerator.appendNumber')}</span>
                    </label>
                </div>
            {/if}

            {#if mode === 'secret'}
                <div class="grid gap-4 md:grid-cols-2">
                    <div>
                        <span class="mb-2 block text-xs uppercase tracking-wider text-muted font-semibold">{$_('tool.passwordGenerator.keyFormat')}</span>
                        <div class="grid grid-cols-3 gap-2">
                            {#each ['raw', 'hex', 'base32', 'base64', 'base64url'] as format}
                                <button type="button"
                                        onclick={() => (secretFormat = format as SecretFormat)}
                                        class="pg-format-tab"
                                        class:is-active={secretFormat === format}
                                        aria-pressed={secretFormat === format}
                                >
                                    {format}
                                </button>
                            {/each}
                        </div>
                        <div class="mt-3">
                            <label for="secret-bytes" class="block text-xs uppercase tracking-wider text-muted font-semibold mb-1">{$_('tool.passwordGenerator.bytes')}</label>
                            <input id="secret-bytes" type="range" min="16" max="64" bind:value={secretBytes} class="w-full accent-emerald-500" />
                            <div class="text-xs text-muted mt-1">{$_('tool.passwordGenerator.bytesCount', { values: { count: secretBytes } })}</div>
                        </div>
                        <div class="mt-3">
                            <label class="pg-option pg-option-wide flex items-center gap-2 text-sm">
                                <input type="checkbox" bind:checked={addSecretPrefix} class="accent-emerald-500" />
                                <span>{$_('tool.passwordGenerator.prefixWithContext')}</span>
                            </label>
                            {#if addSecretPrefix}
                                <input type="text" bind:value={secretPrefix} use:escToClear={() => (secretPrefix = '')} class="mt-2 w-full px-3 py-2 bg-panel-2 border border-border rounded-lg font-mono text-sm" />
                            {/if}
                        </div>
                    </div>

                    <div>
                        <label for="secret-count" class="mb-2 block text-xs uppercase tracking-wider text-muted font-semibold">{$_('tool.passwordGenerator.count')}</label>
                        <input id="secret-count" type="range" min="3" max="20" bind:value={secretOutputCount} class="w-full accent-emerald-500" />
                        <div class="text-xs text-muted mt-1">{$_('tool.passwordGenerator.keysCount', { values: { count: secretOutputCount } })}</div>
                    </div>
                </div>
            {/if}
        </div>
    </div>

    <!-- Toolbar -->
    <div class="pg-toolbar flex flex-wrap items-center gap-3 rounded-2xl border border-border bg-panel p-3 md:p-4">
        <div>
            <label for="batch-count" class="block text-[11px] uppercase tracking-wider text-muted font-semibold mb-1">{$_('tool.passwordGenerator.generate', { values: { count } })}</label>
            <input id="batch-count" type="range" min="1" max="50" bind:value={count} class="w-32 accent-emerald-500" />
        </div>

        <div class="flex-1"></div>

        <button type="button" onclick={() => runGenerator()} class="pg-action-button inline-flex h-10 items-center gap-1.5 rounded-xl border border-border bg-panel-2 px-3 text-sm hover:border-accent/70">
            <RefreshCw class="h-4 w-4" /> {$_('tool.passwordGenerator.newBatch')}
        </button>
        {#if mode === 'random' || mode === 'passphrase'}
            <button type="button" onclick={() => (showPasswords = !showPasswords)} class="pg-action-button pg-visibility-button inline-flex h-10 items-center gap-1.5 rounded-xl border border-border bg-panel-2 px-3 text-sm hover:border-accent/70">
                {#if showPasswords}<EyeOff class="h-4 w-4" /> {$_('tool.passwordGenerator.hide')}{:else}<Eye class="h-4 w-4" /> {$_('tool.passwordGenerator.show')}{/if}
            </button>
        {/if}
        <button type="button" onclick={() => copyAll()} class="pg-copy-button inline-flex h-10 items-center gap-1.5 rounded-xl bg-accent px-4 text-sm font-medium text-accent-contrast hover:bg-accent-hover">
            <Copy class="h-4 w-4" /> {$_('tool.passwordGenerator.copyAll')}
        </button>
    </div>

    <!-- Output -->
    <div class="pg-output rounded-2xl border border-border bg-panel p-4 md:p-5 space-y-4">
        {#if mode === 'secret'}
            <div class="pg-results rounded-xl border border-border bg-panel-2 divide-y divide-border max-h-[60vh] overflow-y-auto">
                {#each secretOutputs as value, i}
                    <button type="button"
                            class="pg-result-row w-full text-left px-3 py-2 font-mono text-sm hover:bg-bg/40 flex items-center gap-2 group transition-colors"
                            onclick={() => copyOne(value, `secret-${i + 1}`)}
                    >
                        <span class="text-muted text-xs w-6 shrink-0">{i + 1}.</span>
                        <span class="flex-1 text-text break-all">{value}</span>
                        <span class="text-[10px] {strengthLabel(Math.max(0, entropy(value))).color} shrink-0">{entropy(value)}b</span>
                        <Copy class="w-3 h-3 text-muted opacity-0 group-hover:opacity-100 shrink-0" />
                    </button>
                {/each}
            </div>
            <div class="pg-audit rounded-xl border border-border bg-panel-2 p-3">
                <label for="audit-input" class="text-xs uppercase tracking-wider text-muted block mb-2 font-semibold">{$_('tool.passwordGenerator.auditToken')}</label>
                <textarea
                        id="audit-input"
                        bind:value={secretAuditInput}
                        placeholder={$_('tool.passwordGenerator.auditPlaceholder')}
                        rows="4"
                        class="w-full bg-panel border border-border rounded-lg p-2 text-xs font-mono focus:outline-none focus:border-accent"
                ></textarea>
                {#if secretAuditInput.trim()}
                    <div class="mt-2 text-xs">
                        <div class="flex items-center gap-1.5">
                            <ShieldCheck class="w-3.5 h-3.5 text-accent" />
                            {$_('tool.passwordGenerator.estimatedEntropy')} <span class="font-mono text-text">{$_('tool.passwordGenerator.entropyBits', { values: { count: entropy(secretAuditInput) } })}</span>
                        </div>
                        <div class="mt-2">
                            {#each secretRisk(secretAuditInput) as risk}
                                <div class="text-error">• {$_(risk.key, { values: risk.values })}</div>
                            {/each}
                            {#if secretRisk(secretAuditInput).length === 0}
                                <div class="text-success">{$_('tool.passwordGenerator.noRiskDetected')}</div>
                            {/if}
                        </div>
                    </div>
                {/if}
            </div>
        {:else}
            {#if passwords.length > 0}
                {#if mode === 'random'}
                    {@const first = passwords[0] ?? ''}
                    <div class="pg-strength flex items-center gap-3 text-xs">
                        <span class="text-muted">{$_('tool.passwordGenerator.strength')}</span>
                        <span class="{strengthLabel(entropy(first)).color} font-semibold">{$_(strengthLabel(entropy(first)).label)}</span>
                        <span class="text-muted">{$_('tool.passwordGenerator.bitsEntropy', { values: { count: entropy(first) } })}</span>
                    </div>
                {:else if mode === 'passphrase'}
                    {@const bits = passphraseEntropy()}
                    <div class="pg-strength flex items-center gap-3 text-xs">
                        <span class="text-muted">{$_('tool.passwordGenerator.strength')}</span>
                        <span class="{strengthLabel(bits).color} font-semibold">{$_(strengthLabel(bits).label)}</span>
                        <span class="text-muted">{$_('tool.passwordGenerator.bitsEntropy', { values: { count: bits } })}</span>
                    </div>
                {/if}
                <div class="pg-results rounded-xl border border-border bg-panel-2 divide-y divide-border max-h-[60vh] overflow-y-auto">
                    {#each passwords as p, i}
                        <button type="button"
                                class="pg-result-row w-full text-left px-3 py-2 font-mono text-sm hover:bg-bg/40 flex items-center gap-2 group transition-colors"
                                onclick={() => copyOne(p, `password-${i + 1}`)}
                        >
                            <span class="text-muted text-xs w-6 shrink-0">{i + 1}.</span>
                            <span class="flex-1 text-text break-all">
                                {#if showPasswords || mode !== 'random'}{p}{:else}
                                    {'•'.repeat(Math.min(p.length, 32))}
                                {/if}
                            </span>
                            {#if mode === 'random'}
                                <span class="text-[10px] {strengthLabel(entropy(p)).color} shrink-0">{entropy(p)}b</span>
                            {/if}
                            <Copy class="w-3 h-3 text-muted opacity-0 group-hover:opacity-100 shrink-0" />
                        </button>
                    {/each}
                </div>
            {/if}
        {/if}
    </div>
    </div>
</ToolPage>

<style>
    .pg-workspace {
        display: grid;
        grid-template-areas:
            'config toolbar'
            'config output';
        grid-template-columns: minmax(250px, 0.8fr) minmax(0, 1.6fr);
        gap: 0 24px;
        min-width: 0;
    }

    .pg-config {
        grid-area: config;
        min-width: 0;
        border: 0;
        border-right: 1px solid var(--color-border);
        border-radius: 0;
        padding: 0 24px 0 0;
        background: transparent;
    }

    .pg-toolbar {
        grid-area: toolbar;
        min-width: 0;
        border: 0;
        border-bottom: 1px solid var(--color-border);
        border-radius: 0;
        padding: 0 0 12px;
        background: transparent;
    }

    .pg-output {
        display: grid;
        grid-area: output;
        align-content: start;
        min-width: 0;
        border: 0;
        border-radius: 0;
        padding: 16px 0 0;
        background: transparent;
    }

    .pg-mode-tabs {
        gap: 4px;
        border-bottom-color: var(--color-border);
        padding-bottom: 10px;
    }

    .pg-mode-tab,
    .pg-format-tab {
        position: relative;
        min-height: 34px;
        border: 1px solid transparent;
        border-radius: var(--radius-control);
        padding: 0 12px 0 16px;
        color: var(--color-text-secondary);
        background: transparent;
        font: inherit;
        font-size: 0.8125rem;
        transition: color var(--dur-micro) var(--ease-out), background var(--dur-micro) var(--ease-out), border-color var(--dur-micro) var(--ease-out);
    }

    .pg-mode-tab:hover,
    .pg-format-tab:hover,
    .pg-action-button:hover {
        color: var(--color-text);
        border-color: color-mix(in srgb, var(--color-text) 14%, transparent);
    }

    .pg-mode-tab.is-active,
    .pg-format-tab.is-active {
        color: var(--color-text);
        background: var(--color-panel-2);
        border-color: color-mix(in srgb, var(--color-text) 8%, transparent);
    }

    .pg-mode-tab.is-active::before,
    .pg-format-tab.is-active::before,
    .pg-option:has(input:checked)::before {
        position: absolute;
        top: 7px;
        bottom: 7px;
        left: 5px;
        width: 3px;
        border-radius: 999px;
        background: var(--color-accent);
        content: '';
    }

    .pg-config :global(label[for]) {
        color: var(--color-muted);
        font-size: 0.6875rem;
        font-weight: 650;
        letter-spacing: 0.07em;
        text-transform: uppercase;
    }

    .pg-config input[type='range'],
    .pg-toolbar input[type='range'] {
        accent-color: var(--color-accent);
    }

    .pg-config input[type='text'],
    .pg-audit textarea {
        border-color: var(--color-border);
        border-radius: var(--radius-control);
        background: var(--color-panel);
    }

    .pg-config input[type='text']:focus,
    .pg-audit textarea:focus,
    .pg-mode-tab:focus-visible,
    .pg-format-tab:focus-visible,
    .pg-action-button:focus-visible,
    .pg-copy-button:focus-visible,
    .pg-result-row:focus-visible,
    .pg-option:has(input:focus-visible) {
        border-color: var(--color-accent);
        box-shadow: 0 0 0 3px color-mix(in srgb, var(--color-accent) 18%, transparent);
        outline: none;
    }

    .pg-option {
        position: relative;
        min-height: 38px;
        overflow: hidden;
        border-color: var(--color-border);
        padding-left: 14px;
        color: var(--color-text-secondary);
        background: var(--color-panel);
        transition: color var(--dur-micro) var(--ease-out), background var(--dur-micro) var(--ease-out), border-color var(--dur-micro) var(--ease-out);
    }

    .pg-option:hover {
        color: var(--color-text);
        border-color: color-mix(in srgb, var(--color-text) 14%, transparent);
    }

    .pg-option:has(input:checked) {
        color: var(--color-text);
        background: var(--color-panel-2);
    }

    .pg-option input {
        z-index: 1;
        accent-color: var(--color-accent);
    }

    .pg-option-wide {
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        padding: 9px 10px 9px 14px;
    }

    .pg-format-tab {
        width: 100%;
        overflow: hidden;
        padding-right: 8px;
        padding-left: 13px;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .pg-action-button {
        border-color: var(--color-border);
        color: var(--color-text-secondary);
        background: var(--color-panel);
        transition: color var(--dur-micro) var(--ease-out), border-color var(--dur-micro) var(--ease-out), background var(--dur-micro) var(--ease-out);
    }

    .pg-copy-button {
        border: 1px solid transparent;
        transition: background var(--dur-micro) var(--ease-out), box-shadow var(--dur-micro) var(--ease-out);
    }

    .pg-copy-button:focus-visible {
        box-shadow: 0 0 0 3px color-mix(in srgb, var(--color-accent) 22%, transparent);
        outline: none;
    }

    .pg-strength {
        flex-wrap: wrap;
        gap: 7px;
        border-bottom: 1px solid var(--color-border);
        padding-bottom: 11px;
    }

    .pg-results {
        border-width: 1px 0;
        border-radius: 0;
        background: transparent;
    }

    .pg-result-row {
        position: relative;
        border-bottom-color: var(--color-border);
        padding: 10px 8px;
        background: transparent;
        transition: background var(--dur-micro) var(--ease-out);
    }

    .pg-result-row:hover {
        background: var(--color-panel-2);
    }

    .pg-result-row:focus-visible {
        position: relative;
        z-index: 1;
    }

    .pg-audit {
        border-width: 0 0 0 2px;
        border-color: color-mix(in srgb, var(--color-accent) 60%, transparent);
        border-radius: 0;
        padding: 4px 0 4px 12px;
        background: transparent;
    }

    .pg-audit :global(label[for]) {
        color: var(--color-muted);
        font-size: 0.6875rem;
        font-weight: 650;
        letter-spacing: 0.07em;
        text-transform: uppercase;
    }

    @media (max-width: 820px) {
        .pg-workspace {
            grid-template-areas:
                'toolbar'
                'config'
                'output';
            grid-template-columns: minmax(0, 1fr);
            gap: 18px;
        }

        .pg-config {
            border-right: 0;
            border-bottom: 1px solid var(--color-border);
            padding: 0 0 18px;
        }

        .pg-toolbar {
            padding-bottom: 12px;
        }

        .pg-output {
            padding-top: 0;
        }
    }

    @media (max-width: 620px) {
        .pg-toolbar {
            align-items: flex-start;
        }

        .pg-toolbar :global(.flex-1) {
            display: none;
        }

        .pg-visibility-button {
            padding-right: 10px;
            padding-left: 10px;
        }

        .pg-config :global(.grid-cols-3) {
            grid-template-columns: repeat(2, minmax(0, 1fr));
        }
    }
</style>
