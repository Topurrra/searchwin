<script lang="ts">
    /* Tag chip input for the note editor.
     *
     * The .ki frontmatter has carried a `tags` field all along, and search has
     * always matched against it — there was simply no way to put a tag on a
     * note. This is that missing half; the round-trip below it already works.
     *
     * Commas are a commit key, not a character: the frontmatter tag list is
     * split on ',' in three separate places (notes.ts, notes.rs parse_tags,
     * preview.ts) with a naive split that doesn't honour quoting, so a tag
     * containing a comma would silently tear in two on reload. Making ','
     * commit the chip means one can never be created. */
    import { X } from '@lucide/svelte';

    let {
        tags,
        suggestions,
        onChange,
    }: {
        tags: string[];
        suggestions: string[];
        onChange: (tags: string[]) => void;
    } = $props();

    let draft = $state('');
    let open = $state(false);
    let highlightIndex = $state(-1);
    let inputEl: HTMLInputElement | null = $state(null);

    /** Prefix match on what's typed, minus tags already on this note. */
    const MAX_SUGGESTIONS = 8;
    let filteredSuggestions = $derived.by(() => {
        const q = draft.trim().toLowerCase();
        if (!q) return [];
        const onNote = new Set(tags.map((t) => t.toLowerCase()));
        return suggestions
            .filter((s) => s.toLowerCase().startsWith(q) && !onNote.has(s.toLowerCase()))
            .slice(0, MAX_SUGGESTIONS);
    });

    /* The filtered list shrinks as you type, so an index from a longer list can
     * outlive it and point at nothing (or the wrong row). Clamp on every
     * change rather than trusting the keyboard handler to have kept up. */
    $effect(() => {
        if (highlightIndex >= filteredSuggestions.length) {
            highlightIndex = filteredSuggestions.length - 1;
        }
    });

    function commit(raw: string) {
        const candidate = raw.trim();
        if (!candidate) return;
        // Case-insensitive dedupe: without it 'Work'/'work'/'WORK' fork into
        // three tags over time and the autocomplete stops being useful.
        if (tags.some((t) => t.toLowerCase() === candidate.toLowerCase())) {
            draft = '';
            open = false;
            return;
        }
        onChange([...tags, candidate]);
        draft = '';
        open = false;
        highlightIndex = -1;
    }

    function remove(tag: string) {
        onChange(tags.filter((t) => t !== tag));
    }

    function onKeydown(e: KeyboardEvent) {
        if (e.key === 'Enter' || e.key === ',') {
            e.preventDefault();
            // Prefer the highlighted suggestion so its existing casing wins —
            // that's what keeps the tag vocabulary from fragmenting.
            commit(
                highlightIndex >= 0 && filteredSuggestions[highlightIndex]
                    ? filteredSuggestions[highlightIndex]
                    : draft,
            );
            return;
        }
        if (e.key === 'Backspace' && draft === '' && tags.length) {
            e.preventDefault();
            onChange(tags.slice(0, -1));
            return;
        }
        if (e.key === 'ArrowDown' && filteredSuggestions.length) {
            e.preventDefault();
            open = true;
            highlightIndex = Math.min(highlightIndex + 1, filteredSuggestions.length - 1);
            return;
        }
        if (e.key === 'ArrowUp' && filteredSuggestions.length) {
            e.preventDefault();
            highlightIndex = Math.max(highlightIndex - 1, -1);
            return;
        }
        if (e.key === 'Escape' && open) {
            // Only swallow Escape when the dropdown is actually open, so it
            // still reaches the editor's own handlers otherwise.
            e.preventDefault();
            e.stopPropagation();
            open = false;
            highlightIndex = -1;
        }
    }
</script>

<div class="tag-input">
    {#each tags as tag (tag)}
        <span class="tag-chip">
            {tag}
            <button
                type="button"
                class="tag-chip-x"
                onclick={() => remove(tag)}
                aria-label={`Remove tag ${tag}`}
            >
                <X class="tag-chip-x-ico" />
            </button>
        </span>
    {/each}

    <div class="tag-field">
        <input
            bind:this={inputEl}
            bind:value={draft}
            class="tag-entry"
            type="text"
            placeholder={tags.length ? 'Add tag…' : 'Add a tag…'}
            aria-label="Add a tag"
            autocomplete="off"
            oninput={() => {
                open = true;
                highlightIndex = -1;
            }}
            onfocus={() => (open = true)}
            onblur={() => (open = false)}
            onkeydown={onKeydown}
        />

        {#if open && filteredSuggestions.length}
            <ul class="tag-menu" role="listbox">
                {#each filteredSuggestions as s, i (s)}
                    <li>
                        <button
                            type="button"
                            class="tag-opt"
                            class:is-active={i === highlightIndex}
                            role="option"
                            aria-selected={i === highlightIndex}
                            onmousedown={(e) => {
                                /* mousedown, not click: blur fires first and
                                 * would close the menu before click landed. */
                                e.preventDefault();
                                commit(s);
                            }}
                            onmouseenter={() => (highlightIndex = i)}
                        >
                            {s}
                        </button>
                    </li>
                {/each}
            </ul>
        {/if}
    </div>
</div>

<style>
    .tag-input {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 6px;
    }
    /* Chip idiom — matches the read-only note tags the palette already shows.
       Deliberately NOT the selected-row pattern: these are chips, not rows. */
    .tag-chip {
        display: inline-flex;
        align-items: center;
        gap: 4px;
        padding: 2px 4px 2px 7px;
        background: color-mix(in srgb, var(--color-accent) 9%, transparent);
        color: color-mix(in srgb, var(--color-accent) 80%, var(--color-text));
        border-radius: 6px;
        font-size: 10px;
        font-weight: 500;
    }
    .tag-chip-x {
        display: inline-flex;
        align-items: center;
        padding: 0;
        border: none;
        background: none;
        color: inherit;
        cursor: pointer;
        opacity: 0;
        transition: opacity var(--dur-micro, 120ms) var(--ease-out, ease);
    }
    /* Reveal on chip hover or keyboard focus — focus matters or the × is
       unreachable without a mouse. */
    .tag-chip:hover .tag-chip-x,
    .tag-chip-x:focus-visible {
        opacity: 1;
    }
    .tag-chip :global(.tag-chip-x-ico) {
        width: 11px;
        height: 11px;
    }
    .tag-field {
        position: relative;
        flex: 1;
        min-width: 90px;
    }
    .tag-entry {
        width: 100%;
        padding: 2px 0;
        border: none;
        background: none;
        color: var(--color-text);
        font-size: 11.5px;
    }
    .tag-entry::placeholder {
        color: var(--color-muted);
    }
    .tag-entry:focus {
        outline: none;
    }
    .tag-menu {
        position: absolute;
        top: calc(100% + 4px);
        left: 0;
        z-index: 20;
        min-width: 160px;
        max-height: 210px;
        overflow-y: auto;
        margin: 0;
        padding: 4px;
        list-style: none;
        background: var(--color-panel);
        border: 1px solid var(--color-border);
        border-radius: 10px;
        box-shadow: var(--shadow-md, 0 8px 24px rgb(0 0 0 / 0.28));
    }
    .tag-opt {
        position: relative;
        display: block;
        width: 100%;
        padding: 5px 10px 5px 14px;
        border: none;
        border-radius: 6px;
        background: none;
        color: var(--color-text-secondary);
        font-size: 11.5px;
        text-align: left;
        cursor: pointer;
    }
    /* Canonical selected-item pattern: neutral panel-2 fill + accent pill
       strip + accent text at regular weight. NOT an accent-tinted row. */
    .tag-opt.is-active {
        background: var(--color-panel-2);
        color: var(--color-accent);
    }
    .tag-opt.is-active::before {
        content: '';
        position: absolute;
        left: 3px;
        top: 6px;
        bottom: 6px;
        width: 3px;
        border-radius: 999px;
        background: var(--color-accent);
    }
</style>
