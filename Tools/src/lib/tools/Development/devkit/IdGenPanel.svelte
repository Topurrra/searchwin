<script lang="ts">
    import { Copy, RefreshCw, Hash } from '@lucide/svelte';
    import { toast } from '$lib/stores/toasts';
    import { escToClear } from '$lib/actions/escToClear';
    import { Button, ToolPanel, Checkbox, EmptyState } from '$lib/ui';

    type IdType = 'uuid-v4' | 'uuid-v1-like' | 'ulid' | 'nanoid' | 'nanoid-custom';

    let idType = $state<IdType>('uuid-v4');
    let count = $state(5);
    let nanoAlphabet = $state('ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789');
    let nanoSize = $state(21);
    let uppercase = $state(false);
    let ids = $state<string[]>([]);

    const types: { id: IdType; name: string; description: string; example: string }[] = [
        { id: 'uuid-v4',       name: 'UUID v4',       description: 'Random UUID. Most common standard.',    example: '550e8400-e29b-41d4-a716-446655440000' },
        { id: 'uuid-v1-like',  name: 'UUID v1-like',  description: 'Time-prefixed UUID (fake timestamp).',  example: '1ae5c000-e29b-11ee-a716-446655440000' },
        { id: 'ulid',          name: 'ULID',           description: 'Sortable, time-prefixed, Crockford32.',  example: '01ARZ3NDEKTSV4RRFFQ69G5FAV' },
        { id: 'nanoid',        name: 'NanoID',         description: 'URL-safe, compact, collision-resistant.', example: 'V1StGXR8_Z5jdHi6B-myT' },
        { id: 'nanoid-custom', name: 'NanoID custom',  description: 'Custom alphabet and length.',             example: 'abc123XYZ' },
    ];

    // --- Generators ---
    function genUuidV4(): string {
        return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, c => {
            const r = crypto.getRandomValues(new Uint8Array(1))[0] & 15;
            const v = c === 'x' ? r : (r & 0x3 | 0x8);
            return v.toString(16);
        });
    }

    function genUuidV1Like(): string {
        const now = Date.now();
        const timeHex = now.toString(16).padStart(12, '0');
        const rand = (len: number) => Array.from(crypto.getRandomValues(new Uint8Array(len))).map(b => b.toString(16).padStart(2, '0')).join('');
        return `${timeHex.slice(4)}-${timeHex.slice(0, 4)}-11${rand(1)}-${(0x80 | (crypto.getRandomValues(new Uint8Array(1))[0] & 0x3f)).toString(16)}${rand(1)}-${rand(6)}`;
    }

    // Crockford Base32 alphabet
    const CROCKFORD = '0123456789ABCDEFGHJKMNPQRSTVWXYZ';

    function genUlid(): string {
        const now = Date.now();
        let timeStr = '';
        let t = now;
        for (let i = 9; i >= 0; i--) {
            timeStr = CROCKFORD[t % 32] + timeStr;
            t = Math.floor(t / 32);
        }
        let randStr = '';
        const randBytes = crypto.getRandomValues(new Uint8Array(10));
        let val = 0n;
        for (const b of randBytes) val = (val << 8n) | BigInt(b);
        for (let i = 15; i >= 0; i--) {
            randStr = CROCKFORD[Number(val % 32n)] + randStr;
            val = val >> 5n;
        }
        return timeStr + randStr;
    }

    function genNanoId(alphabet: string, size: number): string {
        const arr = crypto.getRandomValues(new Uint8Array(size * 2));
        let id = '';
        for (let i = 0; i < arr.length && id.length < size; i++) {
            const idx = arr[i] % alphabet.length;
            id += alphabet[idx];
        }
        return id.slice(0, size);
    }

    function generate() {
        ids = Array.from({ length: count }, () => {
            let id: string;
            switch (idType) {
                case 'uuid-v4':       id = genUuidV4(); break;
                case 'uuid-v1-like':  id = genUuidV1Like(); break;
                case 'ulid':          id = genUlid(); break;
                case 'nanoid':        id = genNanoId('ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-', 21); break;
                case 'nanoid-custom': id = genNanoId(nanoAlphabet || 'abcdef0123456789', nanoSize); break;
            }
            return uppercase ? id.toUpperCase() : id;
        });
    }

    $effect(() => {
        idType; count; nanoAlphabet; nanoSize; uppercase;
        generate();
    });

    async function copyAll() {
        await navigator.clipboard.writeText(ids.join('\n'));
        toast(`Copied ${ids.length} IDs`, 'success');
    }

    async function copyOne(id: string) {
        await navigator.clipboard.writeText(id);
        toast('Copied', 'success');
    }

    let currentType = $derived(types.find(t => t.id === idType)!);
</script>

<div class="dt-panel">
    <div class="dt-head">
        <div class="dt-head-text">
            <h2 class="dt-title">ID Generator — UUID, ULID &amp; NanoID</h2>
            <p class="dt-desc">Generate UUIDs, ULIDs, and NanoIDs in bulk with format options, then copy them. Generated locally.</p>
        </div>
    </div>

    <div class="dt-grid">
        <!-- Generated IDs -->
        <div class="dt-col">
            <div class="dt-toolbar">
                <span class="dt-section-label">Generated IDs</span>
                <div class="dt-toolbar-end">
                    <Button size="sm" variant="secondary" icon={RefreshCw} onclick={generate}>Regenerate</Button>
                    <Button size="sm" variant="primary" icon={Copy} onclick={copyAll} disabled={ids.length === 0}>Copy all</Button>
                </div>
            </div>

            {#if ids.length === 0}
                <EmptyState
                    icon={Hash}
                    variant="dashed"
                    title="No IDs yet"
                    description="Pick a format and count, then regenerate to produce fresh IDs locally."
                />
            {:else}
                <div class="dt-results">
                    {#each ids as id, i}
                        <button
                            type="button"
                            class="dt-result dt-result-row"
                            onclick={() => copyOne(id)}
                            title="Click to copy"
                        >
                            <span class="dt-text-muted" style="flex:none; width:1.75rem;">{i + 1}.</span>
                            <span class="dt-result-name dt-mono">{id}</span>
                            <Copy class="dt-result-copy-ico" aria-hidden="true" />
                        </button>
                    {/each}
                </div>

                <p class="dt-hint">
                    Click any ID to copy it. {ids.length} generated · {currentType.description}
                </p>
            {/if}
        </div>

        <!-- Options -->
        <ToolPanel padding="md">
            <div class="dt-col">
                <div class="dt-field">
                    <span class="dt-label">Format</span>
                    <div class="dt-seg" role="group" aria-label="ID format">
                        {#each types as t}
                            <button
                                type="button"
                                class="dt-seg-btn"
                                class:is-active={idType === t.id}
                                aria-pressed={idType === t.id}
                                title={t.description}
                                onclick={() => (idType = t.id)}
                            >
                                {t.name}
                            </button>
                        {/each}
                    </div>
                </div>

                {#if idType === 'nanoid-custom'}
                    <div class="dt-fields cols-2">
                        <label class="dt-field">
                            <span class="dt-label">Alphabet</span>
                            <input
                                type="text"
                                class="dt-input dt-mono"
                                bind:value={nanoAlphabet}
                                use:escToClear={() => (nanoAlphabet = '')}
                            />
                        </label>
                        <label class="dt-field">
                            <span class="dt-label">Length: {nanoSize}</span>
                            <input type="range" min="4" max="64" bind:value={nanoSize} />
                        </label>
                    </div>
                {/if}

                <label class="dt-field">
                    <span class="dt-label">Count: {count}</span>
                    <input type="range" min="1" max="100" bind:value={count} />
                </label>

                <Checkbox bind:checked={uppercase} label="Uppercase" />
            </div>
        </ToolPanel>
    </div>
</div>

<style>
    .dt-result-row :global(.dt-result-copy-ico) {
        flex: none;
        width: 14px;
        height: 14px;
        color: var(--color-muted);
        opacity: 0;
        transition: opacity var(--dur-micro, 120ms) var(--ease-out, ease);
    }
    button.dt-result-row {
        cursor: pointer;
        text-align: left;
    }
    button.dt-result-row:hover {
        background: color-mix(in srgb, var(--color-text) 5%, var(--color-panel-2));
    }
    button.dt-result-row:hover :global(.dt-result-copy-ico) {
        opacity: 1;
    }
</style>
