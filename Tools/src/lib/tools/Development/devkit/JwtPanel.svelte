<script lang="ts">
    import { AlertTriangle, CheckCircle, Copy, FileText, RefreshCw, ShieldCheck, XCircle } from '@lucide/svelte';
    import { toast } from '$lib/stores/toasts';
    import { errorToast } from '$lib/stores/errorToast';
    import EmptyState from '$lib/components/EmptyState.svelte';
    import { Button, ToolPanel } from '$lib/ui';

    type Mode = 'decode' | 'verify';
    type Decoded = {
        header: Record<string, unknown> | null;
        payload: Record<string, unknown> | null;
        signature: string;
        rawHeader: string;
        rawPayload: string;
        error: string | null;
    };
    type VerifyState = 'idle' | 'running' | 'valid' | 'invalid' | 'error';

    let mode = $state<Mode>('decode');
    let input = $state('eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyLCJleHAiOjk5OTk5OTk5OTl9.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c');
    let showSignature = $state(true);
    let keyMode = $state<'text' | 'pem'>('text');
    let verifyKey = $state('');
    let verifyState = $state<VerifyState>('idle');
    let verifyMessage = $state('');
    let verifyDuration = $state(0);

    const encoder = new TextEncoder();

    function base64UrlDecode(raw: string): string {
        const normalized = raw.replace(/-/g, '+').replace(/_/g, '/');
        const padded = normalized + '='.repeat((4 - normalized.length % 4) % 4);
        try {
            return decodeURIComponent(
                atob(padded)
                    .split('')
                    .map((c) => '%' + c.charCodeAt(0).toString(16).padStart(2, '0'))
                    .join(''),
            );
        } catch {
            return '';
        }
    }

    function base64UrlToBytes(raw: string): Uint8Array {
        const normalized = raw.replace(/-/g, '+').replace(/_/g, '/');
        const padded = normalized + '='.repeat((4 - normalized.length % 4) % 4);
        const binary = atob(padded);
        const out = new Uint8Array(binary.length);
        for (let i = 0; i < binary.length; i++) {
            out[i] = binary.charCodeAt(i);
        }
        return out;
    }

    function stripPem(pem: string): string {
        return pem
            .replace(/-----BEGIN [^-]+-----/g, '')
            .replace(/-----END [^-]+-----/g, '')
            .replace(/\s+/g, '');
    }

    function pemToBytes(pem: string): Uint8Array {
        const cleaned = stripPem(pem);
        const normalized = cleaned + '='.repeat((4 - cleaned.length % 4) % 4);
        const binary = atob(normalized);
        const out = new Uint8Array(binary.length);
        for (let i = 0; i < binary.length; i++) {
            out[i] = binary.charCodeAt(i);
        }
        return out;
    }

    function derLength(value: number): number[] {
        if (value < 128) return [value];
        const out: number[] = [];
        let n = value;
        while (n > 0) {
            out.unshift(n & 0xff);
            n >>= 8;
        }
        return [0x80 | out.length, ...out];
    }

    function encodeInteger(raw: Uint8Array): number[] {
        let start = 0;
        while (start < raw.length - 1 && raw[start] === 0 && (raw[start + 1] & 0x80) === 0) {
            start++;
        }
        const normalized = raw.slice(start);
        const payload = normalized[0] >= 0x80 ? new Uint8Array([0, ...normalized]) : normalized;
        return [0x02, ...derLength(payload.length), ...Array.from(payload)];
    }

    function joseToDer(sig: Uint8Array, alg: string): Uint8Array {
        let expected = 64;
        if (alg === 'ES384') expected = 96;
        if (alg === 'ES512') expected = 132;
        if (sig.length !== expected) {
            throw new Error(`Expected ${expected} bytes for ${alg} signature`);
        }
        const half = expected / 2;
        const r = sig.slice(0, half);
        const s = sig.slice(half);
        const first = encodeInteger(r);
        const second = encodeInteger(s);
        const body = [...first, ...second];
        return new Uint8Array([0x30, ...derLength(body.length), ...body]);
    }

    function algFamily(alg: string): 'hmac' | 'rsa' | 'ecdsa' | 'unsupported' {
        if (alg.startsWith('HS')) return 'hmac';
        if (alg.startsWith('RS')) return 'rsa';
        if (alg.startsWith('ES')) return 'ecdsa';
        return 'unsupported';
    }

    function algHash(alg: string): 'SHA-256' | 'SHA-384' | 'SHA-512' {
        if (alg.includes('384')) return 'SHA-384';
        if (alg.includes('512')) return 'SHA-512';
        return 'SHA-256';
    }

    let decoded = $derived.by((): Decoded => {
        const trimmed = input.trim();
        if (!trimmed) return { header: null, payload: null, signature: '', rawHeader: '', rawPayload: '', error: null };

        const parts = trimmed.split('.');
        if (parts.length !== 3) {
            return { header: null, payload: null, signature: '', rawHeader: '', rawPayload: '', error: 'JWT must have 3 parts separated by dots' };
        }

        const headerRaw = base64UrlDecode(parts[0]);
        const payloadRaw = base64UrlDecode(parts[1]);
        if (!headerRaw || !payloadRaw) {
            return { header: null, payload: null, signature: '', rawHeader: '', rawPayload: '', error: 'Unable to decode base64url payload' };
        }

        try {
            const header = JSON.parse(headerRaw) as Record<string, unknown>;
            const payload = JSON.parse(payloadRaw) as Record<string, unknown>;
            return { header, payload, signature: parts[2], rawHeader: headerRaw, rawPayload: payloadRaw, error: null };
        } catch (error) {
            return { header: null, payload: null, signature: '', rawHeader: '', rawPayload: '', error: `Decode failed: ${error}` };
        }
    });

    let claims = $derived.by(() => {
        if (!decoded.payload) return [];
        const now = Math.floor(Date.now() / 1000);
        const out: { key: string; value: unknown; note?: string; warn?: boolean }[] = [];

        for (const [k, v] of Object.entries(decoded.payload)) {
            let note: string | undefined;
            let warn = false;

            if (k === 'exp' && typeof v === 'number') {
                const delta = v - now;
                const date = new Date(v * 1000).toLocaleString();
                if (delta < 0) {
                    note = `Expired ${Math.abs(delta)} seconds ago (${date})`;
                    warn = true;
                } else {
                    note = `Expires in ${Math.abs(delta)} seconds (${date})`;
                }
            }
            if (k === 'nbf' && typeof v === 'number') {
                const date = new Date(v * 1000).toLocaleString();
                if (v > now) {
                    note = `Not valid until ${date}`;
                    warn = true;
                } else {
                    note = `Valid since ${date}`;
                }
            }
            if (k === 'iat' && typeof v === 'number') note = `Issued at ${new Date(v * 1000).toLocaleString()}`;
            out.push({ key: k, value: v, note, warn });
        }
        return out;
    });

    function decodeAlgorithmHint(): string {
        const alg = String(decoded.header?.alg ?? '');
        const family = algFamily(alg);
        if (family === 'hmac') return 'Use text secret';
        if (family === 'rsa' || family === 'ecdsa') return 'Use PEM public key';
        return 'Unsupported algorithm';
    }

    function canVerify(): boolean {
        if (!decoded.header) return false;
        if (!verifyKey.trim()) return false;
        if (decoded.error) return false;
        return algFamily(String(decoded.header.alg ?? '')) !== 'unsupported';
    }

    let decodedError = $derived(decoded.error);
    let alg = $derived(String(decoded.header?.alg ?? ''));
    let family = $derived(algFamily(alg));
    let keyMismatchNote = $derived.by(() => {
        if (!input.trim()) return 'Paste a token first';
        if (!decoded.error) return '';
        return decoded.error;
    });

    async function verify() {
        if (decoded.error) {
            verifyState = 'error';
            verifyMessage = decoded.error;
            return;
        }
        if (!canVerify()) {
            verifyState = 'error';
            verifyMessage = keyMismatchNote || 'Cannot verify this token';
            return;
        }
        verifyState = 'running';
        verifyMessage = 'Verifying...';
        verifyDuration = 0;
        const start = performance.now();

        try {
            const parts = input.trim().split('.');
            const signingInput = `${parts[0]}.${parts[1]}`;
            const signature = base64UrlToBytes(parts[2]);
            const hash = algHash(alg);

            if (family === 'hmac') {
                const key = await crypto.subtle.importKey(
                    'raw',
                    encoder.encode(verifyKey),
                    { name: 'HMAC', hash },
                    false,
                    ['verify'],
                );
                const valid = await crypto.subtle.verify({ name: 'HMAC', hash }, key, signature, encoder.encode(signingInput));
                verifyState = valid ? 'valid' : 'invalid';
            } else if (family === 'rsa') {
                if (keyMode === 'text') {
                    throw new Error('RSA signature verification requires PEM public key format');
                }
                const key = await crypto.subtle.importKey(
                    'spki',
                    pemToBytes(verifyKey),
                    { name: 'RSASSA-PKCS1-v1_5', hash },
                    false,
                    ['verify'],
                );
                const valid = await crypto.subtle.verify(
                    { name: 'RSASSA-PKCS1-v1_5', hash },
                    key,
                    signature,
                    encoder.encode(signingInput),
                );
                verifyState = valid ? 'valid' : 'invalid';
            } else {
                if (keyMode === 'text') throw new Error('ECDSA verification requires PEM public key format');
                const derSig = joseToDer(signature, alg);
                const curve = alg === 'ES256' ? 'P-256' : alg === 'ES384' ? 'P-384' : 'P-521';
                const key = await crypto.subtle.importKey(
                    'spki',
                    pemToBytes(verifyKey),
                    { name: 'ECDSA', namedCurve: curve },
                    false,
                    ['verify'],
                );
                const valid = await crypto.subtle.verify({ name: 'ECDSA', hash }, key, derSig, encoder.encode(signingInput));
                verifyState = valid ? 'valid' : 'invalid';
            }

            verifyDuration = Math.round(performance.now() - start);
            verifyMessage = verifyState === 'valid' ? 'Signature is valid' : 'Signature does not match';
            if (verifyState === 'valid') {
                toast('Signature verified', 'success');
            } else if (verifyState === 'invalid') {
                toast('Signature invalid', 'error');
            }
        } catch (error) {
            verifyState = 'error';
            verifyDuration = Math.round(performance.now() - start);
            verifyMessage = String(error);
            // UxAudit UX-F-01: was `toast('Verification failed', 'error')`
            // — two words, no reason. Now: situation + likely cause.
            errorToast("Couldn't verify this token", error, {
                hint: 'Check that you pasted the public key in PEM or JWK format and that the algorithm matches the token header.',
            });
        }
    }

    function loadSample() {
        input = 'eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyLCJleHAiOjk5OTk5OTk5OTl9.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c';
        if (mode === 'verify') {
            verifyState = 'idle';
            verifyMessage = '';
            verifyDuration = 0;
        }
    }

    function fillExampleKey() {
        keyMode = 'text';
        verifyKey = 'super-secret';
    }

    async function copy(text: string, label: string) {
        await navigator.clipboard.writeText(text);
        toast(`Copied ${label}`, 'success');
    }

    function clear() {
        input = '';
        verifyState = 'idle';
        verifyMessage = '';
        verifyDuration = 0;
    }

    function resultColor() {
        if (verifyState === 'valid') return 'dt-text-success';
        if (verifyState === 'invalid') return 'dt-text-warning';
        if (verifyState === 'error') return 'dt-text-error';
        if (verifyState === 'running') return '';
        return 'dt-text-muted';
    }
</script>

<div class="dt-panel">
    <div class="dt-head">
        <div class="dt-head-text">
            <h2 class="dt-title">JWT Decoder — inspect & verify tokens locally</h2>
            <p class="dt-desc">Decode claims, see expirations, and verify HMAC, RSA, or ECDSA signatures locally via the Web Crypto API. Tokens never leave your machine.</p>
        </div>
        <div class="dt-seg" role="group" aria-label="Mode">
            <button class="dt-seg-btn" class:is-active={mode === 'decode'} onclick={() => (mode = 'decode')}>Decoder</button>
            <button class="dt-seg-btn" class:is-active={mode === 'verify'} onclick={() => (mode = 'verify')}>Verify</button>
        </div>
    </div>

    <!-- Toolbar (actions) -->
    <div class="dt-toolbar">
        <Button variant="secondary" onclick={loadSample}>Sample</Button>
        <Button variant="ghost" onclick={clear}>Clear</Button>
        {#if mode === 'verify'}
            <div class="dt-toolbar-end">
                <Button variant="primary" icon={RefreshCw} disabled={!input.trim()} onclick={verify}>Verify signature</Button>
            </div>
        {/if}
    </div>

    <ToolPanel padding="md">
        <div class="dt-two">
            <div class="dt-io">
                <div class="dt-io-head">
                    <span class="dt-section-label">JWT</span>
                </div>
                <textarea
                    class="dt-ta is-tall"
                    bind:value={input}
                    spellcheck="false"
                    placeholder="Paste a JWT here (xxx.yyy.zzz)..."
                ></textarea>
            </div>

            <div class="dt-col">
                <div class="dt-io-head">
                    <span class="dt-section-label">Decoded + Verify</span>
                </div>

                {#if mode === 'verify'}
                    <ToolPanel padding="sm" tone="panel-2">
                        <div class="dt-col">
                            <div class="dt-toolbar">
                                <div class="dt-kv">
                                    <span class="dt-kv-key">Algorithm</span>
                                    <span class="dt-kv-val dt-mono">{alg || 'unknown'}</span>
                                </div>
                                <div class="dt-toolbar-end">
                                    <span class={resultColor()} style="font-size: 12px;">{verifyMessage || 'Not run'}{verifyDuration ? ` (${verifyDuration}ms)` : ''}</span>
                                </div>
                            </div>
                            <div class="dt-hint">{decodeAlgorithmHint()}</div>
                            <div class="dt-toolbar">
                                <div class="dt-seg" role="group" aria-label="Key type">
                                    <button class="dt-seg-btn" class:is-active={keyMode === 'text'} onclick={() => (keyMode = 'text')}>Shared secret</button>
                                    <button class="dt-seg-btn" class:is-active={keyMode === 'pem'} onclick={() => (keyMode = 'pem')}>PEM</button>
                                </div>
                                <div class="dt-toolbar-end">
                                    <Button variant="ghost" size="sm" onclick={fillExampleKey}>Use sample key</Button>
                                </div>
                            </div>
                            {#if keyMode === 'pem'}
                                <textarea
                                    class="dt-ta dt-mono"
                                    bind:value={verifyKey}
                                    rows="4"
                                    spellcheck="false"
                                    placeholder="-----BEGIN PUBLIC KEY-----...-----END PUBLIC KEY-----"
                                ></textarea>
                            {:else}
                                <input
                                    class="dt-input dt-mono"
                                    bind:value={verifyKey}
                                    type="text"
                                    placeholder="Paste shared secret"
                                />
                            {/if}
                            <div class="dt-kv">
                                <span class={resultColor()} style="display: inline-flex; align-items: center; gap: 6px;">
                                    {#if verifyState === 'valid'}
                                        <CheckCircle class="dt-ico" />
                                    {:else if verifyState === 'invalid'}
                                        <AlertTriangle class="dt-ico" />
                                    {:else if verifyState === 'error'}
                                        <XCircle class="dt-ico" />
                                    {:else}
                                        <ShieldCheck class="dt-ico" />
                                    {/if}
                                    <span>{verifyMessage || 'Awaiting verification'}</span>
                                </span>
                            </div>
                        </div>
                    </ToolPanel>
                {/if}

                {#if decoded.error}
                    <div class="dt-error">{decoded.error}</div>
                {:else if !input.trim()}
                    <EmptyState
                        icon={FileText}
                        title="No token yet"
                        description="Paste a JWT to decode it."
                        variant="compact"
                    />
                {:else if decoded.header && decoded.payload}
                    <div class="dt-col">
                        <div class="dt-io">
                            <div class="dt-io-head">
                                <span class="dt-section-label">Header</span>
                                <Button variant="ghost" size="sm" icon={Copy} onclick={() => copy(JSON.stringify(decoded.header, null, 2), 'header')} aria-label="Copy header">Copy</Button>
                            </div>
                            <pre class="dt-pre">{JSON.stringify(decoded.header, null, 2)}</pre>
                        </div>

                        <div class="dt-io">
                            <div class="dt-io-head">
                                <span class="dt-section-label">Payload</span>
                                <Button variant="ghost" size="sm" icon={Copy} onclick={() => copy(JSON.stringify(decoded.payload, null, 2), 'payload')} aria-label="Copy payload">Copy</Button>
                            </div>
                            <pre class="dt-pre">{JSON.stringify(decoded.payload, null, 2)}</pre>

                            {#if claims.length > 0}
                                <div class="dt-io">
                                    <div class="dt-io-head">
                                        <span class="dt-section-label">Claims summary</span>
                                    </div>
                                    {#each claims as c}
                                        <div class="dt-kv">
                                            <span class="dt-kv-key">{c.key}</span>
                                            <span class="dt-kv-val">{typeof c.value === 'object' ? JSON.stringify(c.value) : String(c.value)}</span>
                                            {#if c.note}
                                                <span class="dt-kv-note" class:is-warn={c.warn}>{c.note}</span>
                                            {/if}
                                        </div>
                                    {/each}
                                </div>
                            {/if}
                        </div>

                        <div class="dt-io">
                            <div class="dt-io-head">
                                <span class="dt-section-label">Signature</span>
                                <div class="dt-toolbar-end">
                                    <Button variant="ghost" size="sm" onclick={() => (showSignature = !showSignature)}>{showSignature ? 'Hide' : 'Show'}</Button>
                                    <Button variant="ghost" size="sm" icon={Copy} onclick={() => copy(decoded.signature, 'signature')} aria-label="Copy signature">Copy</Button>
                                </div>
                            </div>
                            <div class="dt-mono dt-text-muted" style="font-size: 12px; word-break: break-all;">
                                {showSignature ? decoded.signature : '•'.repeat(Math.min(decoded.signature.length, 64))}
                            </div>
                        </div>
                    </div>
                {/if}
            </div>
        </div>
    </ToolPanel>
</div>

<style>
    /* Local-only: size the inline status icons. dt-* classes stay
       global (owned by the parent shell) and are never redefined here. */
    .dt-panel :global(.dt-ico) {
        width: 14px;
        height: 14px;
        flex: none;
    }
</style>
