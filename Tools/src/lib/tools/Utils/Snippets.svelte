<script lang="ts">
    /*
      Snippets management screen — Phase 3.1 redesign.

      Backend / behavior preserved EXACTLY:
        - Same snippet store wiring (`refreshSnippets`, `createSnippet`,
          `updateSnippet`, `deleteSnippet`, `previewSnippet`).
        - Same i18n keys — every $_('tool.snippets.*') string is reused.
        - Same edit-modal behavior: trigger / label / template, token
          palette, live preview, insert-at-caret token insertion.
        - Same toast pattern on create / update / delete / preview.
        - Same confirm() prompt before delete.

      Surface changes:
        - Hero + 2×2 stats DROPPED. Replaced with the calm ToolPage
          header (compact icon + title + 1-line description + "New"
          action). Snippet count is implicit (the list IS the count).
        - Hand-rolled toolbar replaced with ToolToolbar + kit TextInput.
        - Loading / empty / no-match states routed through ResultList
          (consistent across every pillar).
        - Per-snippet row stays custom (its anatomy — trigger chip +
          template preview + per-row actions — is too specific for the
          generic ResultRow). Wrapped in a ToolPanel for the surface.
        - Editor modal stays a modal (creating/editing a discrete
          artifact, not a setting). Re-themed to the new conventions:
          kit Buttons for actions, scrim without backdrop-filter
          (architectural-rule-safe — DesignPro §motion rule 3).
        - Keyboard hint about Ctrl+Shift+V moved into the ToolPage
          footer slot — the new place for keyboard hints.
    */
    import { onMount, untrack } from 'svelte';
    import {
        Code2,
        Plus,
        Pencil,
        Trash2,
        Search,
        X,
        Keyboard,
        FileText,
        Play,
        Hash,
    } from '@lucide/svelte';
    import {
        snippets,
        snippetsLoading,
        refreshSnippets,
        createSnippet,
        updateSnippet,
        deleteSnippet,
        previewSnippet,
        syncSnippetAutoExpand,
        snippetVariables,
        type Snippet,
    } from '$lib/stores/snippets';
    import { invoke } from '@tauri-apps/api/core';
    import { settings } from '$lib/stores/settings';
    import { toast } from '$lib/stores/toasts';
    // Themed in-app confirm — same Promise<boolean> shape as the
    // Tauri plugin's confirm(), but rendered with KeepItLocal's tokens.
    import { confirm } from '$lib/stores/confirmDialog';
    import { _ } from 'svelte-i18n';
    // Phase 1 kit primitives. These plus the existing Button / TextInput
    // are the whole vocabulary of this page — no hand-rolled boxes.
    import {
        ToolPage,
        ToolToolbar,
        ToolPanel,
        ResultList,
        Button,
        Toggle,
        TextInput,
        Kbd,
    } from '$lib/ui';

    /** Editor state. `editing.id === null` means "creating new"; any
     * other id means "editing existing". */
    type EditorState = {
        open: boolean;
        id: number | null;
        trigger: string;
        label: string;
        template: string;
    };
    let editor = $state<EditorState>({
        open: false,
        id: null,
        trigger: '',
        label: '',
        template: '',
    });

    let query = $state('');
    let livePreview = $state('');
    let previewError = $state<string | null>(null);

    /** Refresh the preview whenever the template in the editor changes.
     * Debounced via untrack so we don't spam the backend on every
     * keystroke when typing fast. Preserved 1:1 from the old surface. */
    let previewTimeout: ReturnType<typeof setTimeout> | null = null;
    $effect(() => {
        const tmpl = editor.template;
        const open = editor.open;
        if (!open) return;
        if (previewTimeout) clearTimeout(previewTimeout);
        previewTimeout = setTimeout(async () => {
            try {
                const expanded = await previewSnippet(tmpl, $_('tool.snippets.yourClipboard'));
                untrack(() => {
                    livePreview = expanded;
                    previewError = null;
                });
            } catch (error) {
                untrack(() => {
                    previewError = String(error);
                });
            }
        }, 120);
    });

    let filtered = $derived.by(() => {
        const q = query.trim().toLowerCase();
        if (!q) return $snippets;
        return $snippets.filter(
            (s) =>
                s.trigger.toLowerCase().includes(q) ||
                s.label.toLowerCase().includes(q) ||
                s.template.toLowerCase().includes(q),
        );
    });

    onMount(() => {
        void refreshSnippets().catch((error) => {
            toast($_('tool.snippets.couldNotLoad', { values: { error: String(error) } }), 'error');
        });
        // Honor a persisted auto-expand opt-in: re-arm the watcher on launch.
        if ($settings.snippetAutoExpandEnabled) {
            void syncSnippetAutoExpand()
                .then(() => invoke('set_snippet_autoexpand_enabled', { enabled: true }))
                .catch(() => {});
        }
    });

    // ─── Snippet auto-expand (opt-in global text-expander) ─────────────
    let exclInput = $state('');

    async function setAutoExpand(on: boolean) {
        settings.update((s) => ({ ...s, snippetAutoExpandEnabled: on }));
        try {
            if (on) await syncSnippetAutoExpand();
            await invoke('set_snippet_autoexpand_enabled', { enabled: on });
        } catch {
            toast("Couldn't toggle snippet auto-expand", 'error');
        }
    }
    function addExcluded() {
        const name = exclInput.trim().toLowerCase();
        if (!name) return;
        settings.update((s) => {
            const list = s.snippetAutoExpandExcludedApps ?? [];
            if (list.includes(name)) return s;
            return { ...s, snippetAutoExpandExcludedApps: [...list, name] };
        });
        exclInput = '';
        void syncSnippetAutoExpand().catch(() => {});
    }
    function removeExcluded(name: string) {
        settings.update((s) => ({
            ...s,
            snippetAutoExpandExcludedApps: (s.snippetAutoExpandExcludedApps ?? []).filter(
                (a) => a !== name,
            ),
        }));
        void syncSnippetAutoExpand().catch(() => {});
    }

    function openCreate() {
        editor = { open: true, id: null, trigger: '', label: '', template: '' };
        livePreview = '';
        previewError = null;
    }

    function openEdit(snippet: Snippet) {
        editor = {
            open: true,
            id: snippet.id,
            trigger: snippet.trigger,
            label: snippet.label,
            template: snippet.template,
        };
        previewError = null;
    }

    function closeEditor() {
        editor.open = false;
    }

    async function save() {
        const trigger = editor.trigger.trim();
        const label = editor.label.trim();
        const template = editor.template;
        if (!trigger || !label || !template) {
            toast($_('tool.snippets.fieldsRequired'), 'error');
            return;
        }
        try {
            if (editor.id === null) {
                await createSnippet({ trigger, label, template });
                toast($_('tool.snippets.createdSnippet', { values: { trigger } }), 'success');
            } else {
                await updateSnippet({ id: editor.id, trigger, label, template });
                toast($_('tool.snippets.updatedSnippet', { values: { trigger } }), 'success');
            }
            closeEditor();
        } catch (error) {
            toast($_('tool.snippets.saveFailed', { values: { error: String(error) } }), 'error');
        }
    }

    async function remove(snippet: Snippet) {
        const ok = await confirm(
            $_('tool.snippets.confirmDelete', { values: { trigger: snippet.trigger } }),
            { title: $_('tool.snippets.deleteSnippetTitle'), kind: 'warning' },
        );
        if (!ok) return;
        try {
            await deleteSnippet(snippet.id);
            toast($_('tool.snippets.snippetDeleted'), 'success');
        } catch (error) {
            toast($_('tool.snippets.deleteFailed', { values: { error: String(error) } }), 'error');
        }
    }

    /** Preview-expand the snippet against a sample clipboard string so
     * the user can see what their template currently produces without
     * actually pasting it anywhere. Useful for catching typos in the
     * variable syntax. Preserved verbatim. */
    async function testExpand(snippet: Snippet) {
        try {
            const expanded = await previewSnippet(snippet.template, $_('tool.snippets.yourClipboard'));
            toast(
                $_('tool.snippets.previewToast', { values: { text: truncate(expanded, 200) } }),
                'info',
                6000,
            );
        } catch (error) {
            toast($_('tool.snippets.previewFailed', { values: { error: String(error) } }), 'error');
        }
    }

    function truncate(value: string, max: number): string {
        const clean = value.replace(/\s+/g, ' ').trim();
        return clean.length <= max ? clean : clean.slice(0, max) + '…';
    }

    function templatePreview(template: string): string {
        return truncate(template, 120);
    }

    /** Pretty-print the user's CURRENT clipboard-overlay shortcut for
     *  the footer's keyboard hint. Splits into kbd chips. */
    let clipboardShortcutParts = $derived(
        ($settings.clipboardOverlayShortcut ?? 'CommandOrControl+Shift+V')
            .split('+')
            .map((part) => part.trim())
            .filter((p) => p.length > 0)
            .map((p) =>
                p === 'CommandOrControl' || p === 'Control'
                    ? 'Ctrl'
                    : p === 'Meta' || p === 'Super'
                      ? 'Win'
                      : p,
            ),
    );
    let clipboardShortcutLabel = $derived(clipboardShortcutParts.join('+'));

    /** Insert a variable token at the editor textarea's caret. Quick
     * way for users to add `{{date}}` without remembering the syntax.
     * Preserved verbatim from the old surface. */
    let templateInputEl = $state<HTMLTextAreaElement | null>(null);
    function insertToken(token: string) {
        const el = templateInputEl;
        if (!el) {
            editor.template = (editor.template ?? '') + token;
            return;
        }
        const start = el.selectionStart;
        const end = el.selectionEnd;
        const current = editor.template;
        editor.template = current.slice(0, start) + token + current.slice(end);
        const newCaret = start + token.length;
        queueMicrotask(() => {
            el.focus();
            el.setSelectionRange(newCaret, newCaret);
        });
    }

    function formatRelative(ms: number): string {
        const diff = Date.now() - ms;
        if (diff < 60_000) return $_('tool.snippets.justNow');
        if (diff < 3_600_000) return $_('tool.snippets.minutesAgo', { values: { count: Math.floor(diff / 60_000) } });
        if (diff < 86_400_000) return $_('tool.snippets.hoursAgo', { values: { count: Math.floor(diff / 3_600_000) } });
        return new Date(ms).toLocaleDateString();
    }

    /** Escape-to-close on the editor modal. Scoped: only fires while
     *  the editor is open, so it doesn't conflict with other Esc
     *  handlers on the page. */
    function onWindowKey(event: KeyboardEvent) {
        if (event.key === 'Escape' && editor.open) {
            event.preventDefault();
            closeEditor();
        }
    }

    const VARIABLE_TOKENS = [
        '{{name}}',
        '{{date}}',
        '{{time}}',
        '{{datetime}}',
        '{{weekday}}',
        '{{date:YYYY-MM-DD}}',
        '{{clipboard}}',
        '{{cursor}}',
    ];

    // Built-ins above + the user's own {{variables}}, so custom keys are
    // insertable from the same palette.
    let allVariableTokens = $derived([
        ...VARIABLE_TOKENS,
        ...Object.keys($snippetVariables)
            .map((key) => key.trim())
            .filter((key) => key.length > 0)
            .map((key) => `{{${key}}}`),
    ]);

    // Custom-variable editor ({{surname}} → "Smith"). Stored in settings
    // (local-only); previewSnippet expands them everywhere a snippet is shown
    // or pasted, after the built-ins.
    let newVarKey = $state('');
    let newVarValue = $state('');
    let varEntries = $derived(Object.entries($snippetVariables));

    function addVariable() {
        const key = newVarKey.trim();
        if (!key) return;
        snippetVariables.update((v) => ({ ...v, [key]: newVarValue }));
        newVarKey = '';
        newVarValue = '';
    }
    function setVariable(key: string, value: string) {
        snippetVariables.update((v) => ({ ...v, [key]: value }));
    }
    function removeVariable(key: string) {
        snippetVariables.update((v) => {
            const next = { ...v };
            delete next[key];
            return next;
        });
    }
</script>

<svelte:window onkeydown={onWindowKey} />

<ToolPage
    icon={Code2}
    title={$_('tool.snippets.heroTitle')}
    description={$_('tool.snippets.heroDescription')}
    width="wide"
>
    {#snippet actions()}
        <Button variant="primary" icon={Plus} onclick={openCreate}>
            {$_('tool.snippets.newSnippet')}
        </Button>
    {/snippet}

    <ToolToolbar>
        {#snippet left()}
            <TextInput
                bind:value={query}
                clearOnEscape
                placeholder={$_('tool.snippets.searchPlaceholder')}
                icon={Search}
            />
        {/snippet}
        {#snippet right()}
            <span class="meta-count">
                {filtered.length}
                {filtered.length === 1
                    ? $_('tool.snippets.useSingular')
                    : $_('tool.snippets.usePlural')}
            </span>
        {/snippet}
    </ToolToolbar>

    <div class="ax-bar">
        <div class="ax-main">
            <div class="ax-text">
                <div class="ax-title">Expand snippets as you type</div>
                <div class="ax-desc">
                    Type a trigger in any app and it expands inline. Keystrokes are
                    matched on-device only — never saved, logged, or sent anywhere. Off
                    by default.
                </div>
            </div>
            <Toggle
                checked={$settings.snippetAutoExpandEnabled}
                onchange={(v) => void setAutoExpand(v)}
                ariaLabel="Expand snippets as you type"
            />
        </div>
        {#if $settings.snippetAutoExpandEnabled}
            <div class="ax-excl">
                <span class="ax-excl-label">Don't expand in:</span>
                {#each $settings.snippetAutoExpandExcludedApps as app (app)}
                    <span class="ax-chip">
                        {app}
                        <button
                            type="button"
                            class="ax-chip-x"
                            aria-label="Remove {app}"
                            onclick={() => removeExcluded(app)}
                        >
                            <X class="ax-chip-x-ico" />
                        </button>
                    </span>
                {/each}
                <input
                    class="ax-excl-input"
                    bind:value={exclInput}
                    placeholder="app.exe"
                    spellcheck="false"
                    onkeydown={(e) => {
                        if (e.key === 'Enter') addExcluded();
                    }}
                />
            </div>
        {/if}
    </div>

    <ToolPanel padding="sm">
        <ResultList
            loading={$snippetsLoading}
            empty={!$snippetsLoading && filtered.length === 0}
            emptyIcon={FileText}
            emptyTitle={query.trim()
                ? $_('tool.snippets.noMatch', { values: { query } })
                : $_('tool.snippets.noSnippetsYet')}
            emptyDescription={!query.trim()
                ? $_('tool.snippets.emptySuggestion')
                : ''}
        >
            {#snippet emptyAction()}
                {#if !query.trim()}
                    <Button variant="primary" size="sm" icon={Plus} onclick={openCreate}>
                        {$_('tool.snippets.createFirst')}
                    </Button>
                {/if}
            {/snippet}

            {#each filtered as snippet (snippet.id)}
                <!-- Snippet row — macOS list-row anatomy:
                       row head: [lead: trigger chip + label] [trail: count + time + actions]
                       row body: template preview block
                     The lead/trail split makes the right side
                     right-align predictably, no matter how long the
                     label is. -->
                <article class="sn-row">
                    <div class="sn-row-head">
                        <div class="sn-row-lead">
                            <span class="sn-trigger">/{snippet.trigger}</span>
                            <span class="sn-label">{snippet.label}</span>
                        </div>
                        <div class="sn-row-trail">
                            {#if snippet.useCount > 0}
                                <span class="sn-uses">
                                    <Hash class="sn-uses-ico" />
                                    <span>{snippet.useCount}</span>
                                    <span class="sn-uses-suffix">
                                        {snippet.useCount === 1
                                            ? $_('tool.snippets.useSingular')
                                            : $_('tool.snippets.usePlural')}
                                    </span>
                                </span>
                            {/if}
                            <span class="sn-updated">
                                {$_('tool.snippets.updatedRelative', {
                                    values: { time: formatRelative(snippet.updatedAtMs) },
                                })}
                            </span>
                            <div class="sn-actions">
                                <Button
                                    size="sm"
                                    variant="ghost"
                                    iconOnly
                                    icon={Play}
                                    title={$_('tool.snippets.previewExpansion')}
                                    aria-label={$_('tool.snippets.previewExpansion')}
                                    onclick={() => void testExpand(snippet)}
                                />
                                <Button
                                    size="sm"
                                    variant="ghost"
                                    iconOnly
                                    icon={Pencil}
                                    title={$_('tool.snippets.edit')}
                                    aria-label={$_('tool.snippets.edit')}
                                    onclick={() => openEdit(snippet)}
                                />
                                <Button
                                    size="sm"
                                    variant="ghost"
                                    iconOnly
                                    icon={Trash2}
                                    title={$_('tool.snippets.delete')}
                                    aria-label={$_('tool.snippets.delete')}
                                    onclick={() => void remove(snippet)}
                                    class="sn-danger-btn"
                                />
                            </div>
                        </div>
                    </div>
                    <pre class="sn-template">{templatePreview(snippet.template)}</pre>
                </article>
            {/each}
        </ResultList>
    </ToolPanel>

    {#snippet footer()}
        <!-- Keyboard hint strip — replaces the old hero's clipboard-shortcut
             pill. Lives in the ToolPage footer slot where keyboard hints
             belong now. -->
        <span class="hint-line">
            <Keyboard class="hint-ico" />
            <span>{$_('tool.snippets.pressHintBefore')}</span>
            <Kbd keys={clipboardShortcutLabel} />
            <span>{$_('tool.snippets.pressHintAfter')}</span>
        </span>
    {/snippet}
</ToolPage>

<!-- ─── Editor modal ──────────────────────────────────────────────
     Stays a modal — creating / editing a discrete artifact, not a
     setting. Themed to the new conventions: kit Buttons, scrim
     without backdrop-filter (DesignPro §motion rule 3 caution).
     Behavior preserved 1:1: same fields, same token palette, same
     live preview pipeline, same save / cancel handlers. -->
{#if editor.open}
    <div
        class="ed-scrim"
        role="presentation"
        onclick={(event) => {
            if (event.target === event.currentTarget) closeEditor();
        }}
    >
        <div
            class="ed-panel"
            role="dialog"
            aria-modal="true"
            aria-label={editor.id === null
                ? $_('tool.snippets.createSnippetAria')
                : $_('tool.snippets.editSnippetAria')}
            tabindex="-1"
        >
            <header class="ed-head">
                <div>
                    <h2 class="ed-title">
                        {editor.id === null
                            ? $_('tool.snippets.newSnippetTitle')
                            : $_('tool.snippets.editSnippetTitle')}
                    </h2>
                    <p class="ed-sub">{$_('tool.snippets.editorHint')}</p>
                </div>
                <Button
                    size="sm"
                    variant="ghost"
                    iconOnly
                    icon={X}
                    title={$_('tool.snippets.close')}
                    aria-label={$_('tool.snippets.close')}
                    onclick={closeEditor}
                />
            </header>

            <div class="ed-grid">
                <!-- Left: editor fields -->
                <div class="ed-col">
                    <label class="ed-field">
                        <span class="ed-label">{$_('tool.snippets.trigger')}</span>
                        <div class="ed-trigger-row">
                            <span class="ed-trigger-slash">/</span>
                            <input
                                bind:value={editor.trigger}
                                placeholder={$_('tool.snippets.triggerPlaceholder')}
                                class="ed-input ed-input-mono"
                                autocomplete="off"
                                spellcheck="false"
                            />
                        </div>
                    </label>
                    <label class="ed-field">
                        <span class="ed-label">{$_('tool.snippets.label')}</span>
                        <input
                            bind:value={editor.label}
                            placeholder={$_('tool.snippets.labelPlaceholder')}
                            class="ed-input"
                            spellcheck="false"
                        />
                    </label>
                    <label class="ed-field">
                        <span class="ed-label">{$_('tool.snippets.template')}</span>
                        <textarea
                            bind:this={templateInputEl}
                            bind:value={editor.template}
                            placeholder={`Best,\n{{name}}\nSent {{date}}`}
                            rows="10"
                            class="ed-textarea"
                            spellcheck="false"
                        ></textarea>
                    </label>
                </div>

                <!-- Right: variable palette + live preview -->
                <div class="ed-col">
                    <div class="ed-section">
                        <div class="ed-label">{$_('tool.snippets.insertVariable')}</div>
                        <div class="ed-tokens">
                            {#each allVariableTokens as token}
                                <button
                                    type="button"
                                    onclick={() => insertToken(token)}
                                    class="ed-token"
                                    title={$_('tool.snippets.insertTokenTitle', {
                                        values: { token },
                                    })}
                                >
                                    {token}
                                </button>
                            {/each}
                        </div>
                    </div>

                    <div class="ed-section">
                        <div class="ed-label">Custom variables</div>
                        <div class="ed-vars">
                            {#each varEntries as [key, value] (key)}
                                <div class="ed-var-row">
                                    <code class="ed-var-key">{`{{${key}}}`}</code>
                                    <input
                                        class="ed-var-input"
                                        value={value}
                                        oninput={(e) =>
                                            setVariable(
                                                key,
                                                (e.currentTarget as HTMLInputElement).value,
                                            )}
                                        placeholder="value"
                                    />
                                    <button
                                        type="button"
                                        class="ed-var-btn"
                                        onclick={() => removeVariable(key)}
                                        title="Delete variable"
                                        aria-label="Delete variable"
                                    >
                                        <Trash2 size={13} />
                                    </button>
                                </div>
                            {/each}
                            <div class="ed-var-row">
                                <input
                                    class="ed-var-input ed-var-keyinput"
                                    bind:value={newVarKey}
                                    placeholder="surname"
                                    onkeydown={(e) => e.key === 'Enter' && addVariable()}
                                />
                                <input
                                    class="ed-var-input"
                                    bind:value={newVarValue}
                                    placeholder="Smith"
                                    onkeydown={(e) => e.key === 'Enter' && addVariable()}
                                />
                                <button
                                    type="button"
                                    class="ed-var-btn"
                                    onclick={addVariable}
                                    title="Add variable"
                                    aria-label="Add variable"
                                >
                                    <Plus size={13} />
                                </button>
                            </div>
                        </div>
                    </div>

                    <div class="ed-section">
                        <div class="ed-label">Live preview</div>
                        <div class="ed-preview">
                            {#if previewError}
                                <div class="ed-preview-error">{previewError}</div>
                            {:else if livePreview}
                                <pre class="ed-preview-text">{livePreview}</pre>
                            {:else}
                                <div class="ed-preview-empty">
                                    Type a template to see the preview…
                                </div>
                            {/if}
                        </div>
                    </div>

                    <div class="ed-tip">
                        <strong>Tip:</strong> use
                        <code class="ed-tip-code">{`{{date:YYYY-MM-DD}}`}</code>
                        for custom date formats. Supported tokens: YYYY, YY, MM, DD,
                        HH, mm, ss.
                    </div>
                </div>
            </div>

            <footer class="ed-foot">
                <Button variant="ghost" onclick={closeEditor}>Cancel</Button>
                <Button variant="primary" onclick={() => void save()}>
                    {editor.id === null ? 'Create snippet' : 'Save changes'}
                </Button>
            </footer>
        </div>
    </div>
{/if}

<style>
    /* ─── Meta count next to the search ─────────────────────────── */
    .meta-count {
        font-size: 11.5px;
        color: var(--color-muted);
        font-variant-numeric: tabular-nums;
        white-space: nowrap;
    }

    /* ─── Snippet rows ─────────────────────────────────────────────
       macOS list-row feel: at rest the row is invisible, on hover it
       picks up a subtle background tint (NO border change — macOS
       rows lean on bg shift alone; double-signaling reads jumpy). */
    .sn-row {
        display: flex;
        flex-direction: column;
        gap: 8px;
        padding: 11px 12px;
        border-radius: var(--radius-control);
        background: transparent;
        transition: background-color var(--dur-micro) var(--ease-out);
    }
    .sn-row:hover {
        background: color-mix(in srgb, var(--color-text) 4%, transparent);
    }
    .sn-row + .sn-row {
        margin-top: 2px;
    }

    /* Head row uses a 2-column grid so the trail (count + time + actions)
       right-aligns predictably regardless of label width. The lead
       takes the remaining space and wraps on narrow widths. */
    .sn-row-head {
        display: grid;
        grid-template-columns: 1fr auto;
        column-gap: 12px;
        align-items: center;
        min-width: 0;
    }
    .sn-row-lead {
        display: flex;
        align-items: center;
        gap: 10px;
        min-width: 0;
        flex-wrap: wrap;
    }
    .sn-row-trail {
        display: flex;
        align-items: center;
        gap: 8px;
        flex: none;
    }

    /* Trigger chip — `/foo` in accent-tinted mono. Pill-shaped to
       match the macOS chip convention; accent reserved for the
       trigger because the trigger IS the identity of a snippet. */
    .sn-trigger {
        display: inline-flex;
        align-items: center;
        height: 22px;
        padding: 0 10px;
        border-radius: var(--radius-pill);
        border: 1px solid color-mix(in srgb, var(--color-accent) 28%, var(--color-border));
        background: var(--color-accent-soft);
        color: var(--color-accent);
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 11.5px;
        font-weight: 600;
        white-space: nowrap;
        letter-spacing: -0.005em;
    }
    .sn-label {
        font-size: 13.5px;
        font-weight: 500;
        color: var(--color-text);
        letter-spacing: -0.005em;
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    /* Use-count chip — quieter than the trigger, matches macOS
       "count badge" geometry. Only renders when useCount > 0. */
    .sn-uses {
        display: inline-flex;
        align-items: center;
        gap: 4px;
        height: 20px;
        padding: 0 9px;
        border-radius: var(--radius-pill);
        background: var(--color-panel-2);
        color: var(--color-text-secondary);
        font-size: 11px;
        font-variant-numeric: tabular-nums;
        /* No border — macOS quiet count chips lean on bg only. */
    }
    .sn-row :global(.sn-uses-ico) {
        width: 10px;
        height: 10px;
    }
    .sn-uses-suffix {
        color: var(--color-muted);
    }

    .sn-updated {
        font-size: 11.5px;
        color: var(--color-muted);
        font-variant-numeric: tabular-nums;
        white-space: nowrap;
    }

    /* Actions: always faintly visible (0.5 opacity) so the row never
       feels "empty" — intensifies to full on hover. macOS Sonoma+
       pattern: controls are present at low contrast, focus on intent. */
    .sn-actions {
        display: flex;
        align-items: center;
        gap: 2px;
        opacity: 0.5;
        transition: opacity var(--dur-micro) var(--ease-out);
    }
    .sn-row:hover .sn-actions,
    .sn-row:focus-within .sn-actions {
        opacity: 1;
    }
    /* Danger button gets a red tone on hover. */
    :global(.sn-danger-btn:hover) {
        color: var(--color-error) !important;
    }

    /* Template preview — recessed to the page background (not panel-2)
       so it reads as a *quoted* code block under the row, not another
       card stacked inside the row. macOS Quick Look feel. */
    .sn-template {
        margin: 0;
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 11.5px;
        line-height: 1.55;
        color: var(--color-text-secondary);
        white-space: pre-wrap;
        word-break: break-word;
        background: var(--color-bg);
        border: 1px solid var(--color-divider, var(--color-border));
        border-radius: var(--radius-control);
        padding: 9px 11px;
    }

    /* ─── Footer keyboard hint ───────────────────────────────── */
    .hint-line {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        flex-wrap: wrap;
    }
    :global(.hint-ico) {
        width: 13px;
        height: 13px;
        color: var(--color-accent);
    }

    /* ─── Editor modal ────────────────────────────────────────
       Scrim WITHOUT backdrop-filter (DesignPro §motion rule 3 —
       safer cross-theme + cheaper on GPU). Darker rgba scrim gives
       the same "this is modal" cue.

       The panel itself slides up + fades in from a slight offset;
       this is the rare case where opacity-fade is acceptable
       because it's a *static* element appearing, not a directional
       transition (DesignPro rule 6 targets slide transitions, not
       all opacity). */
    .ed-scrim {
        position: fixed;
        inset: 0;
        z-index: 80;
        display: flex;
        align-items: center;
        justify-content: center;
        padding: 24px;
        background: rgba(0, 0, 0, 0.52);
        animation: ed-scrim-in 180ms var(--ease-out, ease) both;
    }
    .ed-panel {
        position: relative;
        width: 100%;
        max-width: 880px;
        max-height: 90vh;
        overflow-y: auto;
        padding: 18px 20px 14px;
        border-radius: 18px;
        border: 1px solid color-mix(in srgb, var(--color-text) 9%, var(--color-border));
        background: var(--color-panel);
        /* Layered macOS look: a soft elevation shadow + an inset
           top-lit highlight that catches the eye like an OS sheet
           opening. The inset rim is the same recipe used on the
           overlay panel — keeps the modal "lit" rather than flat. */
        box-shadow:
            inset 0 1px 0 color-mix(in srgb, var(--color-text) 8%, transparent),
            var(--shadow-lg);
        animation: ed-panel-in 220ms var(--ease-out, ease) both;
    }
    @keyframes ed-scrim-in {
        from {
            opacity: 0;
        }
        to {
            opacity: 1;
        }
    }
    @keyframes ed-panel-in {
        from {
            opacity: 0;
            transform: translate3d(0, 8px, 0) scale(0.99);
        }
        to {
            opacity: 1;
            transform: translate3d(0, 0, 0) scale(1);
        }
    }

    .ed-head {
        display: flex;
        align-items: flex-start;
        justify-content: space-between;
        gap: 16px;
        margin-bottom: 16px;
    }
    .ed-title {
        margin: 0;
        font-size: 17px;
        font-weight: 600;
        letter-spacing: -0.012em;
        color: var(--color-text);
    }
    .ed-sub {
        margin: 4px 0 0;
        font-size: 12.5px;
        line-height: 1.45;
        color: var(--color-text-secondary);
    }

    .ed-grid {
        display: grid;
        grid-template-columns: 1fr 1fr;
        gap: 16px;
    }
    @media (max-width: 720px) {
        .ed-grid {
            grid-template-columns: 1fr;
        }
    }
    .ed-col {
        display: flex;
        flex-direction: column;
        gap: 12px;
        min-width: 0;
    }
    .ed-section {
        display: flex;
        flex-direction: column;
        gap: 8px;
    }

    .ed-field {
        display: flex;
        flex-direction: column;
        gap: 6px;
    }
    .ed-label {
        font-size: 11px;
        font-weight: 600;
        text-transform: uppercase;
        letter-spacing: 0.06em;
        color: var(--color-muted);
    }
    .ed-input {
        width: 100%;
        height: 36px;
        padding: 0 12px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        color: var(--color-text);
        font-size: 13px;
        outline: none;
        transition: border-color var(--dur-micro) var(--ease-out);
    }
    .ed-input:hover {
        border-color: var(--color-border-strong);
    }
    .ed-input:focus {
        border-color: var(--color-accent);
    }
    .ed-input-mono {
        font-family: 'JetBrains Mono', ui-monospace, monospace;
    }
    .ed-trigger-row {
        display: flex;
        align-items: center;
        gap: 8px;
    }
    .ed-trigger-slash {
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        color: var(--color-muted);
    }
    .ed-textarea {
        width: 100%;
        padding: 12px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        color: var(--color-text);
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 12.5px;
        line-height: 1.55;
        resize: vertical;
        outline: none;
        transition: border-color var(--dur-micro) var(--ease-out);
    }
    .ed-textarea:focus {
        border-color: var(--color-accent);
    }

    .ed-tokens {
        display: flex;
        flex-wrap: wrap;
        gap: 6px;
    }
    .ed-token {
        padding: 4px 8px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: 6px;
        color: var(--color-text-secondary);
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 11px;
        cursor: pointer;
        transition:
            background-color var(--dur-micro) var(--ease-out),
            color var(--dur-micro) var(--ease-out),
            border-color var(--dur-micro) var(--ease-out);
    }
    .ed-token:hover {
        background: var(--color-panel-3);
        color: var(--color-text);
        border-color: color-mix(in srgb, var(--color-accent) 35%, var(--color-border));
    }

    .ed-vars {
        display: flex;
        flex-direction: column;
        gap: 6px;
    }
    .ed-var-row {
        display: flex;
        align-items: center;
        gap: 6px;
    }
    .ed-var-key {
        flex: 0 0 auto;
        min-width: 84px;
        max-width: 120px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 11px;
        color: var(--color-text-secondary);
    }
    .ed-var-input {
        flex: 1 1 auto;
        min-width: 0;
        height: 30px;
        padding: 0 10px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        color: var(--color-text);
        font-size: 12.5px;
        outline: none;
        transition: border-color var(--dur-micro) var(--ease-out);
    }
    .ed-var-input:focus {
        border-color: var(--color-accent);
    }
    .ed-var-keyinput {
        flex: 0 0 auto;
        width: 96px;
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 11px;
    }
    .ed-var-btn {
        flex: 0 0 auto;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 30px;
        height: 30px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        color: var(--color-muted);
        cursor: pointer;
        transition:
            background-color var(--dur-micro) var(--ease-out),
            color var(--dur-micro) var(--ease-out),
            border-color var(--dur-micro) var(--ease-out);
    }
    .ed-var-btn:hover {
        background: var(--color-panel-3);
        color: var(--color-text);
        border-color: color-mix(in srgb, var(--color-accent) 35%, var(--color-border));
    }

    .ed-preview {
        min-height: 140px;
        max-height: 260px;
        overflow: auto;
        padding: 10px 12px;
        background: var(--color-bg);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
    }
    .ed-preview-text {
        margin: 0;
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 12px;
        line-height: 1.55;
        color: var(--color-text);
        white-space: pre-wrap;
        word-break: break-word;
    }
    .ed-preview-empty {
        font-size: 12px;
        font-style: italic;
        color: var(--color-muted);
    }
    .ed-preview-error {
        font-size: 12px;
        color: var(--color-error);
    }

    .ed-tip {
        padding: 10px 12px;
        background: var(--color-info-soft);
        border: 1px solid var(--color-info-strong);
        border-radius: var(--radius-control);
        font-size: 11.5px;
        line-height: 1.55;
        color: var(--color-info);
    }
    .ed-tip-code {
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 11px;
        padding: 1px 5px;
        border-radius: 4px;
        background: color-mix(in srgb, var(--color-info) 14%, transparent);
    }

    .ed-foot {
        display: flex;
        align-items: center;
        justify-content: flex-end;
        gap: 8px;
        margin-top: 16px;
        padding-top: 14px;
        border-top: 1px solid var(--color-divider, var(--color-border));
    }

    /* ─── Snippet auto-expand bar ──────────────────────────────────── */
    .ax-bar {
        margin: 0 0 12px;
        padding: 12px 14px;
        background: var(--color-panel);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card, 12px);
        display: flex;
        flex-direction: column;
        gap: 10px;
    }
    .ax-main {
        display: flex;
        align-items: center;
        gap: 16px;
    }
    .ax-text {
        flex: 1;
        min-width: 0;
    }
    .ax-title {
        font-size: 13px;
        font-weight: 600;
        color: var(--color-text);
    }
    .ax-desc {
        margin-top: 2px;
        font-size: 12px;
        color: var(--color-muted);
        line-height: 1.4;
    }
    .ax-excl {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 6px;
        padding-top: 10px;
        border-top: 1px solid color-mix(in srgb, var(--color-border) 55%, transparent);
    }
    .ax-excl-label {
        font-size: 12px;
        color: var(--color-text-secondary);
    }
    .ax-chip {
        display: inline-flex;
        align-items: center;
        gap: 4px;
        font-size: 12px;
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        color: var(--color-text);
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: 999px;
        padding: 2px 6px 2px 10px;
    }
    .ax-chip-x {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 16px;
        height: 16px;
        padding: 0;
        border: none;
        background: transparent;
        color: var(--color-muted);
        cursor: pointer;
        border-radius: 4px;
    }
    .ax-chip-x:hover {
        background: var(--color-border);
        color: var(--color-text);
    }
    .ax-chip-x :global(.ax-chip-x-ico) {
        width: 11px;
        height: 11px;
    }
    .ax-excl-input {
        height: 28px;
        min-width: 120px;
        padding: 0 8px;
        background: var(--color-bg);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        color: var(--color-text);
        font-size: 12px;
        font-family: 'JetBrains Mono', ui-monospace, monospace;
    }
    .ax-excl-input:focus {
        outline: none;
        border-color: color-mix(in srgb, var(--color-accent) 55%, var(--color-border));
    }
</style>
