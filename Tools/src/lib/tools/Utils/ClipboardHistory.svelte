<script lang="ts">
    /*
      Clipboard History — Phase 3.4 redesign.

      The largest pillar (1865 lines in the old surface). Surface
      changes ONLY — every backend invoke, every store call, every
      i18n key, every behavior preserved bytecode-equivalent.

      Preserved EXACTLY:
        - Every backend invoke (get_clipboard_history,
          get_clipboard_exclusions, get_clipboard_paused,
          get_clipboard_retention_days, get_clipboard_images_enabled,
          get_clipboard_image_retention_days,
          set_clipboard_exclusions, set_clipboard_paused,
          set_clipboard_retention_days,
          set_clipboard_images_enabled,
          set_clipboard_image_retention_days,
          reset_clipboard_exclusions_to_defaults,
          copy_clipboard_entry_to_clipboard, pin_clipboard_entry,
          pin_clipboard_entries, delete_clipboard_entry,
          delete_clipboard_entries, clear_clipboard_history,
          label_clipboard_entry, take_clipboard_recovery_notice).
        - `clipboard-history-updated` event listener.
        - Recovery-notice banner (one-time, dismissible).
        - Search query + category filter logic.
        - Pinned + transient split.
        - Bulk selection (Pin / Unpin / Delete / Select all).
        - Per-entry actions (copy, pin, delete, label edit).
        - Image entries: thumbnail + click-to-preview modal.
        - Sensitive content badges.
        - Infinite scroll via IntersectionObserver.
        - Every i18n key ($_('overlay.clipboardTool.*')).
        - Esc-key handling for preview / selection.
        - `emitTo('main', 'navigate-tool', ...)` to Snippets manager.

      Surface changes:
        - Hero + 2x2 stats DROPPED. Calm ToolPage header instead.
        - Inline Settings panel (exclusions / retention / image
          capture) → SideSheet.
        - Inline Snippets panel → SideSheet with same content + a
          "Manage all" button that jumps to the dedicated Snippets
          page (preserves the old navigate-tool emit).
        - Toolbar replaced with ToolToolbar + kit Buttons.
        - Bulk-action bar restyled but functionally identical.
        - Category chips kept (good pattern, sharpened styling).
        - Pinned section and history list use ToolPanel surfaces.
        - Recovery notice uses ErrorState.
        - Image preview modal kept as-is (its UX is fine).
    */
    import { onMount, onDestroy } from 'svelte';
    import { invoke, convertFileSrc } from '@tauri-apps/api/core';
    import { listen, emitTo, type UnlistenFn } from '@tauri-apps/api/event';
    import {
        Clipboard,
        Pin,
        PinOff,
        Copy,
        Trash2,
        Search,
        ShieldAlert,
        AlertTriangle,
        Eraser,
        Settings2,
        Pause,
        Play,
        Globe,
        Mail,
        Folder,
        Braces,
        Palette,
        Hash as HashIcon,
        Code as CodeIcon,
        Type as TypeIcon,
        Calculator,
        Plus,
        X,
        Check,
        RotateCcw,
        Tag,
        Keyboard,
        ZoomIn,
        Sparkles,
        CheckCheck,
    } from '@lucide/svelte';
    import { flip } from 'svelte/animate';
    import { toast } from '$lib/stores/toasts';
    import { humanizeAppName, sourceColor } from '$lib/utils/appNames';
    import ClipboardActionMenu from '$lib/components/ClipboardActionMenu.svelte';
    import { settings } from '$lib/stores/settings';
    import { snippets, refreshSnippets } from '$lib/stores/snippets';
    import { _ } from 'svelte-i18n';
    import { t } from '$lib/i18n';
    // Themed in-app confirm — drop-in for the Tauri plugin's
    // confirm(), styled to match KeepItLocal's tokens.
    import { confirm } from '$lib/stores/confirmDialog';
    // Phase 1 kit primitives.
    import {
        ToolPage,
        ToolToolbar,
        ToolPanel,
        ResultList,
        Button,
        TextInput,
        SideSheet,
        Kbd,
        ErrorState,
    } from '$lib/ui';

    /** Pretty form of the user's CURRENT clipboard-overlay shortcut.
     *  Used by the footer hint pill — re-derives whenever the setting
     *  changes so remapping the hotkey updates this live, no reload
     *  needed. Splits into kbd chips for nicer display. */
    let clipboardShortcutParts = $derived(
        ($settings.clipboardOverlayShortcut || 'CommandOrControl+Shift+V')
            .split('+')
            .map((part) => part.trim())
            .filter((part) => part.length > 0)
            .map((part) =>
                part === 'CommandOrControl' || part === 'Control'
                    ? 'Ctrl'
                    : part === 'Meta' || part === 'Super'
                      ? 'Win'
                      : part,
            ),
    );
    let clipboardShortcutLabel = $derived(clipboardShortcutParts.join('+'));

    type Category =
        | 'text'
        | 'url'
        | 'email'
        | 'file_path'
        | 'json'
        | 'color'
        | 'hash'
        | 'code'
        | 'number'
        | 'image';

    type EntryKind = 'text' | 'image';

    type ClipboardEntry = {
        id: number;
        capturedAtMs: number;
        kind: EntryKind;
        text: string;
        sourceApp: string | null;
        sensitiveKinds: string[];
        category: Category;
        isPinned: boolean;
        pinLabel: string | null;
        imagePath: string | null;
        thumbnailPath: string | null;
        imageWidth: number | null;
        imageHeight: number | null;
        imageSizeBytes: number | null;
        imageFormat: string | null;
    };

    let entries = $state<ClipboardEntry[]>([]);
    let exclusions = $state<string[]>([]);
    let paused = $state(false);
    let retentionDays = $state(14);
    let imagesEnabled = $state(false);
    let imageRetentionDays = $state(2);
    let query = $state('');
    let activeCategory = $state<Category | 'all'>('all');
    let loading = $state(true);

    // SideSheet toggles (replacing the old inline collapse panels).
    let showSettings = $state(false);
    let showSnippets = $state(false);
    let newExclusion = $state('');

    // Pin-label editing state.
    let editingLabelFor = $state<number | null>(null);
    let labelDraft = $state('');

    /** Full-screen image preview state. Mirrors the same field in the
     *  clipboard overlay so both surfaces feel like one product. */
    let previewEntry = $state<ClipboardEntry | null>(null);

    /** Filename of a quarantined corrupt history file. */
    let recoveryNotice = $state<string | null>(null);

    /** Multi-select for bulk actions. */
    let selectedIds = $state(new Set<number>());

    let unlistenChanges: UnlistenFn | null = null;

    /** Global keydown — Esc closes the preview modal, or clears the
     *  selection if one exists. Side sheets handle their own Esc. */
    function onKeydown(event: KeyboardEvent) {
        if (event.key !== 'Escape') return;
        if (previewEntry) {
            event.preventDefault();
            previewEntry = null;
        } else if (selectedIds.size > 0) {
            event.preventDefault();
            clearSelection();
        }
    }

    async function refresh() {
        try {
            const [
                entriesNext,
                exclusionsNext,
                pausedNext,
                retentionNext,
                imagesEnabledNext,
                imageRetentionNext,
            ] = await Promise.all([
                invoke<ClipboardEntry[]>('get_clipboard_history'),
                invoke<string[]>('get_clipboard_exclusions'),
                invoke<boolean>('get_clipboard_paused'),
                invoke<number>('get_clipboard_retention_days'),
                invoke<boolean>('get_clipboard_images_enabled'),
                invoke<number>('get_clipboard_image_retention_days'),
            ]);
            entries = entriesNext ?? [];
            exclusions = exclusionsNext ?? [];
            paused = pausedNext;
            retentionDays = retentionNext ?? 14;
            imagesEnabled = imagesEnabledNext;
            imageRetentionDays = imageRetentionNext ?? 2;
        } catch (error) {
            console.warn('clipboard history fetch failed:', error);
        } finally {
            loading = false;
        }
    }

    async function updateImagesEnabled(next: boolean) {
        try {
            await invoke('set_clipboard_images_enabled', { enabled: next });
            toast(
                next
                    ? t('overlay.clipboardTool.imagesEnabledToast')
                    : t('overlay.clipboardTool.imagesDisabledToast'),
                'info',
            );
        } catch (error) {
            toast(t('overlay.clipboardTool.couldNotToggle', { error: String(error) }), 'error');
        }
    }

    async function updateImageRetention(days: number) {
        const clamped = Math.max(0, Math.min(90, Math.round(days)));
        try {
            await invoke('set_clipboard_image_retention_days', { days: clamped });
            toast(
                clamped === 0
                    ? t('overlay.clipboardTool.imageRetentionDisabledToast')
                    : t(
                          clamped === 1
                              ? 'overlay.clipboardTool.imageRetentionSetToastOne'
                              : 'overlay.clipboardTool.imageRetentionSetToastOther',
                          { count: clamped },
                      ),
                'success',
            );
        } catch (error) {
            toast(t('overlay.clipboardTool.couldNotSave', { error: String(error) }), 'error');
        }
    }

    async function updateRetention(days: number) {
        const clamped = Math.max(0, Math.min(365, Math.round(days)));
        try {
            await invoke('set_clipboard_retention_days', { days: clamped });
            toast(
                clamped === 0
                    ? t('overlay.clipboardTool.retentionDisabledToast')
                    : t(
                          clamped === 1
                              ? 'overlay.clipboardTool.retentionSetToastOne'
                              : 'overlay.clipboardTool.retentionSetToastOther',
                          { count: clamped },
                      ),
                'success',
            );
        } catch (error) {
            toast(t('overlay.clipboardTool.couldNotSaveRetention', { error: String(error) }), 'error');
        }
    }

    onMount(() => {
        void refresh();
        void refreshSnippets().catch((error) => {
            console.warn('snippets load failed:', error);
        });
        void invoke<string | null>('take_clipboard_recovery_notice')
            .then((name) => {
                if (name) recoveryNotice = name;
            })
            .catch((error) => {
                console.warn('recovery notice check failed:', error);
            });
        void listen('clipboard-history-updated', () => {
            void refresh();
        }).then((un) => {
            unlistenChanges = un;
        });
    });

    /* ─── Infinite scroll ───────────────────────────────────────── */
    $effect(() => {
        if (!sentinelEl || !listContainerEl) return;
        const sentinel = sentinelEl;
        const observer = new IntersectionObserver(
            (entries) => {
                for (const entry of entries) {
                    if (entry.isIntersecting && visibleCount < filteredTransient.length) {
                        visibleCount = Math.min(
                            visibleCount + PAGE_SIZE,
                            filteredTransient.length,
                        );
                    }
                }
            },
            { root: listContainerEl, rootMargin: '200px 0px 200px 0px', threshold: 0 },
        );
        observer.observe(sentinel);
        return () => observer.disconnect();
    });

    onDestroy(() => {
        if (unlistenChanges) {
            unlistenChanges();
            unlistenChanges = null;
        }
    });

    let pinned = $derived(entries.filter((e) => e.isPinned));
    let transient = $derived(entries.filter((e) => !e.isPinned));

    let filteredTransient = $derived.by(() => {
        let result = transient;
        if (activeCategory !== 'all') {
            result = result.filter((e) => e.category === activeCategory);
        }
        const q = query.trim().toLowerCase();
        if (q) {
            result = result.filter((e) => e.text.toLowerCase().includes(q));
        }
        return result;
    });

    let filteredPinned = $derived.by(() => {
        let result = pinned;
        if (activeCategory !== 'all') {
            result = result.filter((e) => e.category === activeCategory);
        }
        const q = query.trim().toLowerCase();
        if (q) {
            result = result.filter((e) => e.text.toLowerCase().includes(q));
        }
        return result;
    });

    let categoryCounts = $derived.by(() => {
        const counts: Record<string, number> = {};
        for (const entry of transient) {
            counts[entry.category] = (counts[entry.category] ?? 0) + 1;
        }
        return counts;
    });

    const PAGE_SIZE = 10;
    let visibleCount = $state(PAGE_SIZE);
    let visibleTransient = $derived(filteredTransient.slice(0, visibleCount));
    let hasMoreTransient = $derived(visibleCount < filteredTransient.length);
    let sentinelEl = $state<HTMLDivElement | null>(null);
    let listContainerEl = $state<HTMLDivElement | null>(null);

    $effect(() => {
        void query;
        void activeCategory;
        visibleCount = PAGE_SIZE;
    });

    $effect(() => {
        const len = filteredTransient.length;
        if (visibleCount > len) {
            visibleCount = Math.max(PAGE_SIZE, len);
        }
    });

    function formatRelative(ms: number): string {
        const diff = Date.now() - ms;
        if (diff < 5_000) return t('overlay.clipboardTool.justNow');
        if (diff < 60_000) return t('overlay.clipboardTool.secondsAgo', { count: Math.floor(diff / 1000) });
        if (diff < 3_600_000) return t('overlay.clipboardTool.minutesAgo', { count: Math.floor(diff / 60_000) });
        if (diff < 86_400_000) return t('overlay.clipboardTool.hoursAgo', { count: Math.floor(diff / 3_600_000) });
        return new Date(ms).toLocaleDateString();
    }

    function preview(text: string, max = 240): string {
        const collapsed = text.replace(/\s+/g, ' ').trim();
        if (collapsed.length <= max) return collapsed;
        return collapsed.slice(0, max) + '…';
    }

    function lineCount(text: string): number {
        return text.split(/\r?\n/).length;
    }

    function categoryIcon(category: Category) {
        switch (category) {
            case 'url':
                return Globe;
            case 'email':
                return Mail;
            case 'file_path':
                return Folder;
            case 'json':
                return Braces;
            case 'color':
                return Palette;
            case 'hash':
                return HashIcon;
            case 'code':
                return CodeIcon;
            case 'number':
                return Calculator;
            default:
                return TypeIcon;
        }
    }

    function categoryLabel(category: Category | 'all'): string {
        const map: Record<Category | 'all', string> = {
            all: 'overlay.clipboardTool.categoryAll',
            text: 'overlay.clipboardTool.categoryText',
            url: 'overlay.clipboardTool.categoryUrls',
            email: 'overlay.clipboardTool.categoryEmails',
            file_path: 'overlay.clipboardTool.categoryPaths',
            json: 'overlay.clipboardTool.categoryJson',
            color: 'overlay.clipboardTool.categoryColors',
            hash: 'overlay.clipboardTool.categoryHashes',
            code: 'overlay.clipboardTool.categoryCode',
            number: 'overlay.clipboardTool.categoryNumbers',
            image: 'overlay.clipboardTool.categoryImages',
        };
        return t(map[category]);
    }

    const CATEGORY_FILTERS: (Category | 'all')[] = [
        'all',
        'url',
        'email',
        'file_path',
        'json',
        'color',
        'hash',
        'code',
        'number',
        'text',
    ];

    async function copyToClipboard(entry: ClipboardEntry) {
        try {
            // A secret goes back marked, so no clipboard history keeps it.
            await invoke('copy_clipboard_entry_to_clipboard', { id: entry.id, quiet: entry.sensitiveKinds.length > 0 });
            toast(t('overlay.clipboardTool.copiedToast'), 'success');
        } catch (error) {
            toast(t('overlay.clipboardTool.couldNotCopy', { error: String(error) }), 'error');
        }
    }

    async function togglePin(entry: ClipboardEntry) {
        try {
            await invoke('pin_clipboard_entry', { id: entry.id, pinned: !entry.isPinned });
        } catch (error) {
            toast(t('overlay.clipboardTool.couldNotPin', { error: String(error) }), 'error');
        }
    }

    async function deleteEntry(entry: ClipboardEntry) {
        try {
            await invoke('delete_clipboard_entry', { id: entry.id });
        } catch (error) {
            toast(t('overlay.clipboardTool.couldNotDelete', { error: String(error) }), 'error');
        }
    }

    async function clearAll() {
        const transientCount = entries.filter((e) => !e.isPinned).length;
        if (transientCount === 0) {
            toast(t('overlay.clipboardTool.noTransientToClear'), 'info');
            return;
        }
        const ok = await confirm(
            t(
                transientCount === 1
                    ? 'overlay.clipboardTool.clearConfirmOne'
                    : 'overlay.clipboardTool.clearConfirmOther',
                { count: transientCount },
            ),
            { title: t('overlay.clipboardTool.clearConfirmTitle'), kind: 'warning' },
        );
        if (!ok) return;
        try {
            await invoke('clear_clipboard_history');
            toast(t('overlay.clipboardTool.historyClearedToast'), 'success');
        } catch (error) {
            toast(t('overlay.clipboardTool.couldNotClear', { error: String(error) }), 'error');
        }
    }

    async function togglePause() {
        try {
            await invoke('set_clipboard_paused', { paused: !paused });
            toast(
                paused
                    ? t('overlay.clipboardTool.captureResumedToast')
                    : t('overlay.clipboardTool.capturePausedToast'),
                'info',
            );
        } catch (error) {
            toast(t('overlay.clipboardTool.couldNotTogglePause', { error: String(error) }), 'error');
        }
    }

    async function addExclusion() {
        const trimmed = newExclusion.trim();
        if (!trimmed) return;
        if (exclusions.some((e) => e.toLowerCase() === trimmed.toLowerCase())) {
            toast(t('overlay.clipboardTool.alreadyExcludedToast'), 'info');
            return;
        }
        const next = [...exclusions, trimmed];
        try {
            await invoke('set_clipboard_exclusions', { apps: next });
            newExclusion = '';
        } catch (error) {
            toast(t('overlay.clipboardTool.couldNotAdd', { error: String(error) }), 'error');
        }
    }

    async function removeExclusion(name: string) {
        const next = exclusions.filter((e) => e !== name);
        try {
            await invoke('set_clipboard_exclusions', { apps: next });
        } catch (error) {
            toast(t('overlay.clipboardTool.couldNotRemove', { error: String(error) }), 'error');
        }
    }

    async function resetExclusions() {
        const ok = await confirm(
            t('overlay.clipboardTool.resetExclusionsConfirm'),
            { title: t('overlay.clipboardTool.resetExclusionsTitle'), kind: 'warning' },
        );
        if (!ok) return;
        try {
            await invoke('reset_clipboard_exclusions_to_defaults');
            toast(t('overlay.clipboardTool.exclusionsResetToast'), 'success');
        } catch (error) {
            toast(t('overlay.clipboardTool.couldNotReset', { error: String(error) }), 'error');
        }
    }

    function startLabelEdit(entry: ClipboardEntry) {
        editingLabelFor = entry.id;
        labelDraft = entry.pinLabel ?? '';
    }

    async function saveLabel(entryId: number) {
        try {
            await invoke('label_clipboard_entry', {
                id: entryId,
                label: labelDraft.trim() ? labelDraft.trim() : null,
            });
        } catch (error) {
            toast(t('overlay.clipboardTool.couldNotSaveLabel', { error: String(error) }), 'error');
        }
        editingLabelFor = null;
        labelDraft = '';
    }

    function cancelLabelEdit() {
        editingLabelFor = null;
        labelDraft = '';
    }

    function sensitiveLabel(kind: string): string {
        const map: Record<string, string> = {
            sensitive: 'overlay.clipboardTool.sensitiveSensitive',
            aws_access_key: 'overlay.clipboardTool.sensitiveAwsKey',
            github_token: 'overlay.clipboardTool.sensitiveGithubToken',
            slack_token: 'overlay.clipboardTool.sensitiveSlackToken',
            stripe_secret_key: 'overlay.clipboardTool.sensitiveStripeKey',
            google_api_key: 'overlay.clipboardTool.sensitiveGoogleApi',
            twilio_account_sid: 'overlay.clipboardTool.sensitiveTwilioSid',
            sendgrid_api_key: 'overlay.clipboardTool.sensitiveSendgrid',
            jwt_token: 'overlay.clipboardTool.sensitiveJwt',
            openai_api_key: 'overlay.clipboardTool.sensitiveOpenaiKey',
            npm_token: 'overlay.clipboardTool.sensitiveNpmToken',
            bearer_token: 'overlay.clipboardTool.sensitiveBearerToken',
            private_key_pem: 'overlay.clipboardTool.sensitivePrivateKey',
            pgp_private_key: 'overlay.clipboardTool.sensitivePgpKey',
            database_url_with_password: 'overlay.clipboardTool.sensitiveDbUrl',
            ethereum_private_key: 'overlay.clipboardTool.sensitiveEthKey',
            bitcoin_wif_private_key: 'overlay.clipboardTool.sensitiveBtcKey',
            us_ssn: 'overlay.clipboardTool.sensitiveSsn',
            credit_card: 'overlay.clipboardTool.sensitiveCreditCard',
            generic_credential_assignment: 'overlay.clipboardTool.sensitiveCredential',
        };
        const key = map[kind];
        return key ? t(key) : kind;
    }

    function visibleSensitiveKinds(kinds: string[]): string[] {
        return kinds.filter((k) => k !== 'sensitive');
    }

    /* ─── Bulk selection ────────────────────────────────────────── */
    function toggleSelect(id: number) {
        const next = new Set(selectedIds);
        if (next.has(id)) next.delete(id);
        else next.add(id);
        selectedIds = next;
    }

    function clearSelection() {
        selectedIds = new Set();
    }

    function selectAllFiltered() {
        const next = new Set(selectedIds);
        for (const e of filteredPinned) next.add(e.id);
        for (const e of filteredTransient) next.add(e.id);
        selectedIds = next;
    }

    async function bulkDelete() {
        const ids = [...selectedIds];
        if (ids.length === 0) return;
        const ok = await confirm(
            t(
                ids.length === 1
                    ? 'overlay.clipboardTool.bulkDeleteConfirmOne'
                    : 'overlay.clipboardTool.bulkDeleteConfirmOther',
                { count: ids.length },
            ),
            { title: t('overlay.clipboardTool.bulkDeleteConfirmTitle'), kind: 'warning' },
        );
        if (!ok) return;
        try {
            await invoke('delete_clipboard_entries', { ids });
            clearSelection();
            toast(t('overlay.clipboardTool.bulkDeletedToast'), 'success');
        } catch (error) {
            toast(t('overlay.clipboardTool.couldNotDelete', { error: String(error) }), 'error');
        }
    }

    async function bulkSetPinned(pinned: boolean) {
        const ids = [...selectedIds];
        if (ids.length === 0) return;
        try {
            await invoke('pin_clipboard_entries', { ids, pinned });
            clearSelection();
        } catch (error) {
            toast(t('overlay.clipboardTool.couldNotPin', { error: String(error) }), 'error');
        }
    }

    /** Jump to the dedicated Snippets manager screen. The main
     *  window's router listens for `navigate-tool`. */
    function openSnippetsManager() {
        void emitTo('main', 'navigate-tool', { toolId: 'snippets' }).catch((error) => {
            console.warn('navigate to snippets failed:', error);
        });
    }

    /* Drop stale selection ids after a refresh removes the entries. */
    $effect(() => {
        const existing = new Set(entries.map((e) => e.id));
        let stale = false;
        for (const id of selectedIds) {
            if (!existing.has(id)) {
                stale = true;
                break;
            }
        }
        if (stale) {
            selectedIds = new Set([...selectedIds].filter((id) => existing.has(id)));
        }
    });

    /** Open Settings sheet, ensuring the Snippets sheet closes first.
     *  Only one side sheet at a time avoids overlapping panels. */
    function openSettingsSheet() {
        showSnippets = false;
        showSettings = true;
    }
    function openSnippetsSheet() {
        showSettings = false;
        showSnippets = true;
    }

    /** Format image byte count as a short human-readable string.
     *  Preserves the old surface's switch over <1KB / <1MB / else. */
    function formatImageBytes(bytes: number): string {
        if (bytes < 1024) return `${bytes} B`;
        if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
        return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    }
</script>

<svelte:window onkeydown={onKeydown} />

<ToolPage
    icon={Clipboard}
    title={$_('overlay.clipboardTool.heroTitle')}
    description={$_('overlay.clipboardTool.heroDescription')}
    width="wide"
>
    {#snippet actions()}
        <Button
            variant={paused ? 'secondary' : 'ghost'}
            icon={paused ? Play : Pause}
            onclick={() => void togglePause()}
            title={paused
                ? $_('overlay.clipboardTool.resumeCaptureTitle')
                : $_('overlay.clipboardTool.pauseCaptureTitle')}
        >
            {paused ? $_('overlay.clipboardTool.resume') : $_('overlay.clipboardTool.pause')}
        </Button>
        <Button
            variant="ghost"
            icon={Settings2}
            onclick={openSettingsSheet}
            title={$_('overlay.clipboardTool.manageExclusionsTitle')}
        >
            {$_('overlay.clipboardTool.excludedApps')}
        </Button>
    {/snippet}

    <!-- ─── Recovery notice ────────────────────────────────────────
         The backend set this when a corrupt history file was
         quarantined on startup. One-time, dismissible. Uses ErrorState
         (warn-toned via the warning palette) for visual consistency. -->
    {#if recoveryNotice}
        <ErrorState
            title={$_('overlay.clipboardTool.recoveryTitle')}
            description={$_('overlay.clipboardTool.recoveryBody', {
                values: { file: recoveryNotice },
            })}
        >
            {#snippet actions()}
                <Button
                    size="sm"
                    variant="ghost"
                    iconOnly
                    icon={X}
                    title={$_('overlay.clipboardTool.recoveryDismiss')}
                    aria-label={$_('overlay.clipboardTool.recoveryDismiss')}
                    onclick={() => (recoveryNotice = null)}
                />
            {/snippet}
        </ErrorState>
    {/if}

    <!-- ─── Search + actions toolbar ──────────────────────────────── -->
    <ToolToolbar>
        {#snippet left()}
            <TextInput
                bind:value={query}
                clearOnEscape
                placeholder={$_('overlay.clipboardTool.searchPlaceholder')}
                icon={Search}
            />
        {/snippet}
        {#snippet right()}
            <Button
                variant="ghost"
                icon={Sparkles}
                onclick={openSnippetsSheet}
                title={$_('overlay.clipboardTool.manageSnippetsTitle')}
            >
                {$_('overlay.clipboardTool.snippets')}
                <span class="ch-pill-count">{$snippets.length}</span>
            </Button>
            <Button
                variant="ghost"
                icon={Eraser}
                onclick={() => void clearAll()}
                title={$_('overlay.clipboardTool.clearHistoryTitle')}
                class="ch-danger-btn"
            >
                {$_('overlay.clipboardTool.clearHistory')}
            </Button>
        {/snippet}
    </ToolToolbar>

    <!-- ─── Category filter chips ────────────────────────────────── -->
    <div class="ch-cat-row" role="group" aria-label="Category filter">
        <span class="ch-cat-label">{$_('overlay.clipboardTool.filterLabel')}</span>
        {#each CATEGORY_FILTERS as cat}
            {@const Icon = cat === 'all' ? Clipboard : categoryIcon(cat as Category)}
            {@const count = cat === 'all' ? transient.length : (categoryCounts[cat as string] ?? 0)}
            {#if cat === 'all' || count > 0}
                <button
                    type="button"
                    class="ch-cat-chip"
                    class:is-on={activeCategory === cat}
                    onclick={() => (activeCategory = cat)}
                >
                    <Icon class="ch-cat-ico" />
                    <span>{categoryLabel(cat)}</span>
                    {#if count > 0 || cat === 'all'}
                        <span class="ch-cat-count">{count}</span>
                    {/if}
                </button>
            {/if}
        {/each}
    </div>

    <!-- ─── Bulk-action bar (appears when selection > 0) ────────── -->
    {#if selectedIds.size > 0}
        <div class="ch-bulk" role="region" aria-label="Bulk actions">
            <span class="ch-bulk-label">
                <CheckCheck class="ch-bulk-ico" />
                {$_('overlay.clipboardTool.selectedCount', {
                    values: { count: selectedIds.size },
                })}
            </span>
            <div class="ch-bulk-actions">
                <Button size="sm" variant="ghost" icon={CheckCheck} onclick={selectAllFiltered}>
                    {$_('overlay.clipboardTool.selectAll')}
                </Button>
                <Button size="sm" variant="ghost" icon={Pin} onclick={() => void bulkSetPinned(true)}>
                    {$_('overlay.clipboardTool.bulkPin')}
                </Button>
                <Button
                    size="sm"
                    variant="ghost"
                    icon={PinOff}
                    onclick={() => void bulkSetPinned(false)}
                >
                    {$_('overlay.clipboardTool.bulkUnpin')}
                </Button>
                <Button
                    size="sm"
                    variant="danger"
                    icon={Trash2}
                    onclick={() => void bulkDelete()}
                >
                    {$_('overlay.clipboardTool.bulkDelete')}
                </Button>
                <Button size="sm" variant="ghost" icon={X} onclick={clearSelection}>
                    {$_('overlay.clipboardTool.clearSelection')}
                </Button>
            </div>
        </div>
    {/if}

    <!-- ─── Pinned section ─────────────────────────────────────────
         Only mounts when there are pinned entries to show — keeps
         the page calm when nothing is pinned. -->
    {#if filteredPinned.length > 0}
        <ToolPanel padding="sm">
            <header class="ch-section-head">
                <Pin class="ch-section-ico" />
                <span>{$_('overlay.clipboardTool.pinned')}</span>
                <span class="ch-section-count">
                    {filteredPinned.length !== pinned.length
                        ? $_('overlay.clipboardTool.pinnedOf', {
                              values: { visible: filteredPinned.length, total: pinned.length },
                          })
                        : filteredPinned.length}
                </span>
            </header>

            <div class="ch-rows">
                {#each filteredPinned as entry (entry.id)}
                    {@const CatIcon = categoryIcon(entry.category)}
                    <!-- Phase 7.3: physical FLIP reorder on pin/unpin.
                         Slot bounds are recorded before the array re-orders
                         and Svelte animates the difference. Falls back to
                         instant snap when prefers-reduced-motion is set. -->
                    <article
                        class="ch-row ch-row-pinned"
                        class:is-selected={selectedIds.has(entry.id)}
                        animate:flip={{ duration: 220 }}
                    >
                        <input
                            type="checkbox"
                            checked={selectedIds.has(entry.id)}
                            onchange={() => toggleSelect(entry.id)}
                            class="ch-row-check"
                            aria-label={$_('overlay.clipboardTool.selectEntry')}
                        />
                        <div class="ch-row-main">
                            <div class="ch-row-meta">
                                <span class="ch-cat-pill">
                                    <CatIcon class="ch-cat-pill-ico" />
                                    {categoryLabel(entry.category)}
                                </span>
                                <span class="ch-pinned-tag">{$_('overlay.clipboardTool.pinned')}</span>
                                {#if entry.sourceApp}
                                    <span class="ch-meta-dot">·</span>
                                    <span class="ch-source">
                                        <span
                                            class="ch-source-dot"
                                            style="background: {sourceColor(entry.sourceApp)}"
                                        ></span>
                                        <span>
                                            {$_('overlay.clipboardTool.fromApp', {
                                                values: { app: humanizeAppName(entry.sourceApp) },
                                            })}
                                        </span>
                                    </span>
                                {/if}
                                <span class="ch-meta-dot">·</span>
                                <span>
                                    {$_(
                                        lineCount(entry.text) === 1
                                            ? 'overlay.clipboardTool.linesCountOne'
                                            : 'overlay.clipboardTool.linesCountOther',
                                        { values: { count: lineCount(entry.text) } },
                                    )}
                                </span>
                                {#each visibleSensitiveKinds(entry.sensitiveKinds) as kind}
                                    <span class="ch-sensitive">
                                        <ShieldAlert class="ch-sensitive-ico" />
                                        {sensitiveLabel(kind)}
                                    </span>
                                {/each}
                            </div>

                            {#if editingLabelFor === entry.id}
                                <div class="ch-label-row">
                                    <input
                                        bind:value={labelDraft}
                                        placeholder={$_('overlay.clipboardTool.labelInputPlaceholder')}
                                        onkeydown={(e) => {
                                            if (e.key === 'Enter') void saveLabel(entry.id);
                                            if (e.key === 'Escape') cancelLabelEdit();
                                        }}
                                        class="ch-label-input"
                                    />
                                    <Button
                                        size="sm"
                                        variant="primary"
                                        iconOnly
                                        icon={Check}
                                        title={$_('overlay.clipboardTool.saveLabelTitle')}
                                        aria-label={$_('overlay.clipboardTool.saveLabelTitle')}
                                        onclick={() => void saveLabel(entry.id)}
                                    />
                                    <Button
                                        size="sm"
                                        variant="ghost"
                                        iconOnly
                                        icon={X}
                                        title={$_('overlay.clipboardTool.cancel')}
                                        aria-label={$_('overlay.clipboardTool.cancel')}
                                        onclick={cancelLabelEdit}
                                    />
                                </div>
                            {:else if entry.pinLabel}
                                <button
                                    type="button"
                                    onclick={() => startLabelEdit(entry)}
                                    class="ch-label-chip"
                                    title={$_('overlay.clipboardTool.editLabelTitle')}
                                >
                                    <Tag class="ch-label-chip-ico" />
                                    {entry.pinLabel}
                                </button>
                            {:else}
                                <button
                                    type="button"
                                    onclick={() => startLabelEdit(entry)}
                                    class="ch-label-add"
                                    title={$_('overlay.clipboardTool.addLabelTitle')}
                                >
                                    <Tag class="ch-label-add-ico" />
                                    {$_('overlay.clipboardTool.addLabel')}
                                </button>
                            {/if}

                            {#if entry.kind === 'image' && entry.imagePath}
                                <div class="ch-image-row">
                                    <button
                                        type="button"
                                        class="thumb-button"
                                        title={$_('overlay.clipboardTool.clickToPreview')}
                                        onclick={(e) => {
                                            e.stopPropagation();
                                            previewEntry = entry;
                                        }}
                                    >
                                        <img
                                            src={convertFileSrc(
                                                entry.thumbnailPath ?? entry.imagePath,
                                            )}
                                            alt={$_('overlay.clipboardTool.clipboardImage')}
                                            class="image-thumb"
                                            draggable="false"
                                        />
                                        <span class="thumb-zoom-badge" aria-hidden="true">
                                            <ZoomIn class="thumb-zoom-ico" />
                                        </span>
                                    </button>
                                    <div class="ch-image-meta">
                                        {#if entry.imageFormat}
                                            <div class="ch-image-fmt-row">
                                                <span class="ch-image-fmt">
                                                    {entry.imageFormat === 'jpeg'
                                                        ? 'JPEG'
                                                        : entry.imageFormat.toUpperCase()}
                                                </span>
                                                {#if entry.imageFormat === 'gif' || entry.imageFormat === 'webp'}
                                                    <span class="ch-image-animated">
                                                        {$_('overlay.clipboardTool.animated')}
                                                    </span>
                                                {/if}
                                            </div>
                                        {/if}
                                        {#if entry.imageWidth && entry.imageHeight}
                                            <div>
                                                {$_('overlay.clipboardTool.imageDimensions', {
                                                    values: {
                                                        width: entry.imageWidth,
                                                        height: entry.imageHeight,
                                                    },
                                                })}
                                            </div>
                                        {/if}
                                        {#if entry.imageSizeBytes}
                                            <div>{formatImageBytes(entry.imageSizeBytes)}</div>
                                        {/if}
                                    </div>
                                </div>
                            {:else if entry.category === 'color'}
                                <!-- Color entries get an inline swatch
                                     so the value reads as a real color
                                     and not just a hex string. The CSS
                                     engine parses any valid color via
                                     inline background. -->
                                <div class="ch-color-row">
                                    <span
                                        class="ch-color-swatch"
                                        style={`background: ${entry.text.trim()};`}
                                        aria-hidden="true"
                                    ></span>
                                    <pre class="ch-text ch-text-color">{preview(entry.text)}</pre>
                                </div>
                            {:else}
                                <pre class="ch-text">{preview(entry.text)}</pre>
                            {/if}
                        </div>
                        <div class="ch-row-actions">
                            {#if entry.kind === 'text'}
                                <ClipboardActionMenu text={entry.text} category={entry.category} />
                            {/if}
                            <Button
                                size="sm"
                                variant="ghost"
                                iconOnly
                                icon={Copy}
                                title={$_('overlay.clipboardTool.copyBackTitle')}
                                aria-label={$_('overlay.clipboardTool.copyBackTitle')}
                                onclick={() => void copyToClipboard(entry)}
                            />
                            <Button
                                size="sm"
                                variant="ghost"
                                iconOnly
                                icon={PinOff}
                                title={$_('overlay.clipboardTool.unpinTitle')}
                                aria-label={$_('overlay.clipboardTool.unpinTitle')}
                                onclick={() => void togglePin(entry)}
                            />
                            <Button
                                size="sm"
                                variant="ghost"
                                iconOnly
                                icon={Trash2}
                                title={$_('overlay.clipboardTool.deleteTitle')}
                                aria-label={$_('overlay.clipboardTool.deleteTitle')}
                                onclick={() => void deleteEntry(entry)}
                                class="ch-danger-btn"
                            />
                        </div>
                    </article>
                {/each}
            </div>
        </ToolPanel>
    {/if}

    <!-- ─── History list ──────────────────────────────────────────── -->
    <ToolPanel padding="sm">
        <header class="ch-section-head">
            <Clipboard class="ch-section-ico" />
            <span>{$_('overlay.clipboardTool.history')}</span>
            <span class="ch-section-count">
                {hasMoreTransient
                    ? $_('overlay.clipboardTool.historyCounterOf', {
                          values: {
                              visible: visibleTransient.length,
                              total: filteredTransient.length,
                          },
                      })
                    : visibleTransient.length}
            </span>
        </header>

        {#if loading}
            <ResultList loading loadingRows={5} />
        {:else if filteredTransient.length === 0}
            <div class="ch-empty">
                <p class="ch-empty-text">
                    {#if query.trim()}
                        {$_('overlay.clipboardTool.noMatchingEntries', {
                            values: { query },
                        })}
                    {:else if activeCategory !== 'all'}
                        {$_('overlay.clipboardTool.noCategoryEntries', {
                            values: { category: categoryLabel(activeCategory).toLowerCase() },
                        })}
                    {:else if paused}
                        {$_('overlay.clipboardTool.capturePausedEmpty')}
                    {:else}
                        {$_('overlay.clipboardTool.noEntriesYet')}
                    {/if}
                </p>
            </div>
        {:else}
            <div bind:this={listContainerEl} class="ch-rows ch-rows-scroll history-scroll">
                {#each visibleTransient as entry (entry.id)}
                    {@const CatIcon = categoryIcon(entry.category)}
                    <article
                        class="ch-row"
                        class:is-selected={selectedIds.has(entry.id)}
                        animate:flip={{ duration: 220 }}
                    >
                        <input
                            type="checkbox"
                            checked={selectedIds.has(entry.id)}
                            onchange={() => toggleSelect(entry.id)}
                            class="ch-row-check"
                            aria-label={$_('overlay.clipboardTool.selectEntry')}
                        />
                        <div class="ch-row-main">
                            <div class="ch-row-meta">
                                <span class="ch-cat-pill">
                                    <CatIcon class="ch-cat-pill-ico" />
                                    {categoryLabel(entry.category)}
                                </span>
                                <span>{formatRelative(entry.capturedAtMs)}</span>
                                {#if entry.sourceApp}
                                    <span class="ch-meta-dot">·</span>
                                    <span class="ch-source">
                                        <span
                                            class="ch-source-dot"
                                            style="background: {sourceColor(entry.sourceApp)}"
                                        ></span>
                                        <span>
                                            {$_('overlay.clipboardTool.fromApp', {
                                                values: {
                                                    app: humanizeAppName(entry.sourceApp),
                                                },
                                            })}
                                        </span>
                                    </span>
                                {/if}
                                <span class="ch-meta-dot">·</span>
                                <span>
                                    {$_(
                                        lineCount(entry.text) === 1
                                            ? 'overlay.clipboardTool.linesCountOne'
                                            : 'overlay.clipboardTool.linesCountOther',
                                        { values: { count: lineCount(entry.text) } },
                                    )}
                                </span>
                                {#each visibleSensitiveKinds(entry.sensitiveKinds) as kind}
                                    <span class="ch-sensitive">
                                        <ShieldAlert class="ch-sensitive-ico" />
                                        {sensitiveLabel(kind)}
                                    </span>
                                {/each}
                            </div>
                            {#if entry.kind === 'image' && entry.imagePath}
                                <div class="ch-image-row">
                                    <button
                                        type="button"
                                        class="thumb-button"
                                        title={$_('overlay.clipboardTool.clickToPreview')}
                                        onclick={(e) => {
                                            e.stopPropagation();
                                            previewEntry = entry;
                                        }}
                                    >
                                        <img
                                            src={convertFileSrc(
                                                entry.thumbnailPath ?? entry.imagePath,
                                            )}
                                            alt={$_('overlay.clipboardTool.clipboardImage')}
                                            class="image-thumb"
                                            draggable="false"
                                        />
                                        <span class="thumb-zoom-badge" aria-hidden="true">
                                            <ZoomIn class="thumb-zoom-ico" />
                                        </span>
                                    </button>
                                    <div class="ch-image-meta">
                                        {#if entry.imageFormat}
                                            <div class="ch-image-fmt-row">
                                                <span class="ch-image-fmt">
                                                    {entry.imageFormat === 'jpeg'
                                                        ? 'JPEG'
                                                        : entry.imageFormat.toUpperCase()}
                                                </span>
                                                {#if entry.imageFormat === 'gif' || entry.imageFormat === 'webp'}
                                                    <span class="ch-image-animated">
                                                        {$_('overlay.clipboardTool.animated')}
                                                    </span>
                                                {/if}
                                            </div>
                                        {/if}
                                        {#if entry.imageWidth && entry.imageHeight}
                                            <div>
                                                {$_('overlay.clipboardTool.imageDimensions', {
                                                    values: {
                                                        width: entry.imageWidth,
                                                        height: entry.imageHeight,
                                                    },
                                                })}
                                            </div>
                                        {/if}
                                        {#if entry.imageSizeBytes}
                                            <div>{formatImageBytes(entry.imageSizeBytes)}</div>
                                        {/if}
                                    </div>
                                </div>
                            {:else if entry.category === 'color'}
                                <!-- Color entries get an inline swatch
                                     so the value reads as a real color
                                     and not just a hex string. The CSS
                                     engine parses any valid color via
                                     inline background. -->
                                <div class="ch-color-row">
                                    <span
                                        class="ch-color-swatch"
                                        style={`background: ${entry.text.trim()};`}
                                        aria-hidden="true"
                                    ></span>
                                    <pre class="ch-text ch-text-color">{preview(entry.text)}</pre>
                                </div>
                            {:else}
                                <pre class="ch-text">{preview(entry.text)}</pre>
                            {/if}
                        </div>
                        <div class="ch-row-actions">
                            {#if entry.kind === 'text'}
                                <ClipboardActionMenu text={entry.text} category={entry.category} />
                            {/if}
                            <Button
                                size="sm"
                                variant="ghost"
                                iconOnly
                                icon={Copy}
                                title={$_('overlay.clipboardTool.copyBackTitle')}
                                aria-label={$_('overlay.clipboardTool.copyBackTitle')}
                                onclick={() => void copyToClipboard(entry)}
                            />
                            <Button
                                size="sm"
                                variant="ghost"
                                iconOnly
                                icon={Pin}
                                title={$_('overlay.clipboardTool.pinTitle')}
                                aria-label={$_('overlay.clipboardTool.pinTitle')}
                                onclick={() => void togglePin(entry)}
                            />
                            <Button
                                size="sm"
                                variant="ghost"
                                iconOnly
                                icon={Trash2}
                                title={$_('overlay.clipboardTool.deleteTitle')}
                                aria-label={$_('overlay.clipboardTool.deleteTitle')}
                                onclick={() => void deleteEntry(entry)}
                                class="ch-danger-btn"
                            />
                        </div>
                    </article>
                {/each}

                {#if hasMoreTransient}
                    <div bind:this={sentinelEl} class="ch-sentinel">
                        <span class="ch-sentinel-spin" aria-hidden="true"></span>
                        {$_('overlay.clipboardTool.loadingMore', {
                            values: {
                                visible: visibleTransient.length,
                                total: filteredTransient.length,
                            },
                        })}
                    </div>
                {/if}
            </div>
        {/if}
    </ToolPanel>

    {#snippet footer()}
        <span class="ch-footer-line">
            <Keyboard class="ch-footer-ico" />
            <span>{$_('overlay.clipboardTool.heroPressLabel')}</span>
            <Kbd keys={clipboardShortcutLabel} />
            <span>{$_('overlay.clipboardTool.heroSummonHint')}</span>
        </span>
    {/snippet}
</ToolPage>

<!-- ─── SideSheet: Settings (exclusions / retention / images) ──────
     Replaces the old inline collapsible Settings panel. Same controls
     and behaviors. -->
<SideSheet
    open={showSettings}
    title={$_('overlay.clipboardTool.excludedApps')}
    onclose={() => (showSettings = false)}
>
    <div class="ch-sheet-stack">
        <p class="ch-sheet-desc">
            {$_('overlay.clipboardTool.exclusionsDescription', { values: { ext: '.exe' } })}
        </p>

        <!-- Retention setting -->
        <div class="ch-sheet-field">
            <label class="ch-sheet-label" for="ch-retention">
                {$_('overlay.clipboardTool.retentionLabel')}
            </label>
            <div class="ch-sheet-row">
                <input
                    id="ch-retention"
                    type="number"
                    min="0"
                    max="365"
                    value={retentionDays}
                    onchange={(e) => {
                        const next = parseInt(e.currentTarget.value, 10);
                        if (!Number.isNaN(next)) void updateRetention(next);
                    }}
                    class="ch-sheet-input ch-sheet-input-num"
                />
                <span class="ch-sheet-hint">
                    {$_('overlay.clipboardTool.retentionHint', { values: { zero: '0' } })}
                </span>
            </div>
        </div>

        <div class="ch-sheet-divider"></div>

        <!-- Image capture toggle -->
        <div class="ch-sheet-field">
            <label class="ch-sheet-toggle">
                <div class="ch-sheet-toggle-text">
                    <span class="ch-sheet-toggle-label">
                        {$_('overlay.clipboardTool.captureImagesLabel')}
                    </span>
                    <span class="ch-sheet-toggle-hint">
                        {$_('overlay.clipboardTool.captureImagesHint')}
                    </span>
                </div>
                <input
                    type="checkbox"
                    checked={imagesEnabled}
                    onchange={(e) => {
                        imagesEnabled = e.currentTarget.checked;
                        void updateImagesEnabled(e.currentTarget.checked);
                    }}
                    class="ch-sheet-checkbox"
                />
            </label>

            {#if imagesEnabled}
                <div class="ch-sheet-row" style="margin-top: 8px;">
                    <label class="ch-sheet-label" for="ch-img-retention">
                        {$_('overlay.clipboardTool.imageRetentionLabel')}
                    </label>
                    <input
                        id="ch-img-retention"
                        type="number"
                        min="0"
                        max="90"
                        value={imageRetentionDays}
                        onchange={(e) => {
                            const next = parseInt(e.currentTarget.value, 10);
                            if (!Number.isNaN(next)) void updateImageRetention(next);
                        }}
                        class="ch-sheet-input ch-sheet-input-num"
                    />
                    <span class="ch-sheet-hint">
                        {$_('overlay.clipboardTool.imageRetentionHint', {
                            values: { zero: '0' },
                        })}
                    </span>
                </div>
            {/if}
        </div>

        <div class="ch-sheet-divider"></div>

        <!-- Exclusion add field + list -->
        <div class="ch-sheet-field">
            <div class="ch-sheet-label-row">
                <label class="ch-sheet-label" for="ch-excl-input">
                    {$_('overlay.clipboardTool.excludedApps')}
                </label>
                <span class="ch-sheet-count">
                    {$_(
                        exclusions.length === 1
                            ? 'overlay.clipboardTool.appsCountOne'
                            : 'overlay.clipboardTool.appsCountOther',
                        { values: { count: exclusions.length } },
                    )}
                </span>
            </div>
            <div class="ch-sheet-row">
                <input
                    id="ch-excl-input"
                    bind:value={newExclusion}
                    placeholder={$_('overlay.clipboardTool.exclusionInputPlaceholder')}
                    onkeydown={(e) => {
                        if (e.key === 'Enter') {
                            void addExclusion();
                        }
                    }}
                    class="ch-sheet-input"
                />
                <Button
                    variant="primary"
                    size="sm"
                    icon={Plus}
                    onclick={() => void addExclusion()}
                >
                    {$_('overlay.clipboardTool.add')}
                </Button>
            </div>
            {#if exclusions.length === 0}
                <p class="ch-sheet-empty">
                    {$_('overlay.clipboardTool.noAppsExcluded')}
                </p>
            {:else}
                <div class="ch-excl-tags">
                    {#each exclusions as app}
                        <span class="ch-excl-tag">
                            {app}
                            <button
                                type="button"
                                onclick={() => void removeExclusion(app)}
                                class="ch-excl-remove"
                                title={$_('overlay.clipboardTool.removeFromExclusionTitle')}
                                aria-label={$_('overlay.clipboardTool.removeFromExclusionTitle')}
                            >
                                <X class="ch-excl-remove-ico" />
                            </button>
                        </span>
                    {/each}
                </div>
            {/if}
        </div>
    </div>

    {#snippet footer()}
        <Button variant="ghost" icon={RotateCcw} onclick={() => void resetExclusions()}>
            {$_('overlay.clipboardTool.resetToDefaults')}
        </Button>
        <Button variant="primary" onclick={() => (showSettings = false)}>
            {$_('overlay.clipboardTool.cancel') === 'Cancel' ? 'Done' : 'Done'}
        </Button>
    {/snippet}
</SideSheet>

<!-- ─── SideSheet: Snippets — glance view + jump to manager ────── -->
<SideSheet
    open={showSnippets}
    title={$_('overlay.clipboardTool.snippets')}
    onclose={() => (showSnippets = false)}
>
    <div class="ch-sheet-stack">
        <p class="ch-sheet-desc">{$_('overlay.clipboardTool.snippetsDescription')}</p>

        {#if $snippets.length === 0}
            <button
                type="button"
                class="ch-snippets-empty"
                onclick={openSnippetsManager}
                title={$_('overlay.clipboardTool.openSnippetsManagerTitle')}
            >
                <Plus class="ch-snippets-empty-ico" />
                <span>{$_('overlay.clipboardTool.noSnippetsYet')}</span>
            </button>
        {:else}
            <div class="ch-snippets-list">
                {#each $snippets as snippet (snippet.id)}
                    <button
                        type="button"
                        class="ch-snippets-row"
                        onclick={openSnippetsManager}
                        title={$_('overlay.clipboardTool.editSnippetTitle')}
                        animate:flip={{ duration: 220 }}
                    >
                        <Sparkles class="ch-snippets-row-ico" />
                        <span class="ch-snippets-row-text">
                            <span class="ch-snippets-row-label">{snippet.label}</span>
                            <span class="ch-snippets-row-trigger">/{snippet.trigger}</span>
                        </span>
                    </button>
                {/each}
            </div>
        {/if}
    </div>

    {#snippet footer()}
        <Button variant="primary" icon={Settings2} onclick={openSnippetsManager}>
            {$_('overlay.clipboardTool.openSnippetsManager')}
        </Button>
    {/snippet}
</SideSheet>

<!-- ─── Image preview modal — kept as-is, retuned to new tokens ── -->
{#if previewEntry && previewEntry.imagePath}
    <div
        class="ch-preview-backdrop"
        role="presentation"
        onclick={(event) => {
            if (event.target === event.currentTarget) previewEntry = null;
        }}
    >
        <div
            class="ch-preview-stage"
            role="dialog"
            aria-label={$_('overlay.clipboardTool.previewLabel')}
            tabindex="-1"
        >
            <button
                type="button"
                class="ch-preview-close"
                onclick={() => (previewEntry = null)}
                title={$_('overlay.clipboardTool.closePreviewTitle')}
                aria-label={$_('overlay.clipboardTool.closePreview')}
            >
                <X class="ch-preview-close-ico" />
            </button>
            <img
                src={convertFileSrc(previewEntry.imagePath)}
                alt={$_('overlay.clipboardTool.previewImageAlt')}
                class="ch-preview-image"
                draggable="false"
            />
            <div class="ch-preview-meta">
                {#if previewEntry.imageFormat}
                    <span class="ch-preview-tag">
                        {previewEntry.imageFormat === 'jpeg'
                            ? 'JPEG'
                            : previewEntry.imageFormat.toUpperCase()}
                    </span>
                {/if}
                {#if previewEntry.imageFormat === 'gif' || previewEntry.imageFormat === 'webp'}
                    <span class="ch-preview-tag ch-preview-tag-accent">
                        {$_('overlay.clipboardTool.animated')}
                    </span>
                {/if}
                {#if previewEntry.imageWidth && previewEntry.imageHeight}
                    <span class="ch-preview-dim">
                        {previewEntry.imageWidth} × {previewEntry.imageHeight}
                    </span>
                {/if}
                {#if previewEntry.imageSizeBytes}
                    <span class="ch-preview-dim">
                        {formatImageBytes(previewEntry.imageSizeBytes)}
                    </span>
                {/if}
                <span class="ch-preview-hint">{$_('overlay.clipboardTool.previewHint')}</span>
            </div>
        </div>
    </div>
{/if}

<style>
    /* ─── Count pill next to button label ────────────────────── */
    .ch-pill-count {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        min-width: 18px;
        height: 16px;
        margin-left: 4px;
        padding: 0 5px;
        border-radius: var(--radius-pill);
        background: var(--color-panel-3);
        color: var(--color-muted);
        font-size: 10px;
        font-weight: 600;
        font-variant-numeric: tabular-nums;
    }
    /* Danger-toned button (Delete / Clear history) — softens to a
       rose hover via :global since Button.svelte owns its base style. */
    :global(.ch-danger-btn:hover) {
        color: var(--color-error) !important;
    }

    /* ─── Category filter chips ─────────────────────────────── */
    .ch-cat-row {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 8px;
        padding: 12px 16px;
        background: var(--color-panel);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card);
    }
    .ch-cat-label {
        font-size: 11px;
        font-weight: 600;
        text-transform: uppercase;
        letter-spacing: 0.06em;
        color: var(--color-muted);
        margin-right: 4px;
    }
    .ch-cat-chip {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        height: 28px;
        padding: 0 11px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-pill);
        color: var(--color-muted);
        font-size: 12px;
        font-weight: 500;
        cursor: pointer;
        transition:
            background-color var(--dur-micro) var(--ease-out),
            border-color var(--dur-micro) var(--ease-out),
            color var(--dur-micro) var(--ease-out);
    }
    .ch-cat-chip:hover:not(.is-on) {
        border-color: color-mix(in srgb, var(--color-accent) 40%, var(--color-border));
        color: var(--color-text);
    }
    .ch-cat-chip.is-on {
        background: var(--color-accent-soft);
        border-color: color-mix(in srgb, var(--color-accent) 55%, var(--color-border));
        color: var(--color-accent);
    }
    :global(.ch-cat-ico) {
        width: 12px;
        height: 12px;
    }
    .ch-cat-count {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        min-width: 16px;
        height: 14px;
        padding: 0 4px;
        border-radius: var(--radius-pill);
        background: var(--color-panel);
        color: inherit;
        font-size: 10px;
        font-weight: 600;
        font-variant-numeric: tabular-nums;
    }

    /* ─── Bulk-action bar ───────────────────────────────────── */
    .ch-bulk {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 12px;
        padding: 10px 14px;
        background: var(--color-accent-soft);
        border: 1px solid color-mix(in srgb, var(--color-accent) 35%, var(--color-border));
        border-radius: var(--radius-card);
    }
    .ch-bulk-label {
        display: inline-flex;
        align-items: center;
        gap: 8px;
        font-size: 13px;
        font-weight: 500;
        color: var(--color-text);
    }
    :global(.ch-bulk-ico) {
        width: 14px;
        height: 14px;
        color: var(--color-accent);
    }
    .ch-bulk-actions {
        margin-left: auto;
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 6px;
    }

    /* ─── Section headers ───────────────────────────────────── */
    .ch-section-head {
        display: inline-flex;
        align-items: center;
        gap: 8px;
        padding: 6px 8px 10px;
        font-size: 10.5px;
        font-weight: 600;
        text-transform: uppercase;
        letter-spacing: 0.06em;
        color: var(--color-muted);
    }
    :global(.ch-section-ico) {
        width: 13px;
        height: 13px;
        color: var(--color-accent);
    }
    .ch-section-count {
        font-size: 10.5px;
        font-weight: 500;
        font-variant-numeric: tabular-nums;
        text-transform: none;
        letter-spacing: 0;
        color: var(--color-muted);
    }

    /* ─── Entry rows (pinned + history share styling) ───────── */
    .ch-rows {
        display: flex;
        flex-direction: column;
        gap: 6px;
    }
    .ch-rows-scroll {
        max-height: 60vh;
        overflow-y: auto;
        padding-right: 4px;
    }
    .ch-row {
        position: relative;
        display: flex;
        align-items: flex-start;
        gap: 12px;
        padding: 12px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        transition:
            background-color var(--dur-micro) var(--ease-out),
            border-color var(--dur-micro) var(--ease-out);
    }
    .ch-row:hover {
        border-color: color-mix(in srgb, var(--color-accent) 35%, var(--color-border));
    }
    /* Pinned row variant — slight accent tint so pinned items read as
       elevated without using the strong accent-tinted background. */
    .ch-row-pinned {
        background: color-mix(in srgb, var(--color-accent) 5%, var(--color-panel-2));
        border-color: color-mix(in srgb, var(--color-accent) 20%, var(--color-border));
    }
    /* Selected (checked) row — canonical pattern from
       feedback_selected_item_pattern.md: neutral surface + accent pill
       strip via ::before. */
    .ch-row.is-selected {
        background: var(--color-panel-3);
    }
    .ch-row.is-selected::before {
        content: '';
        position: absolute;
        left: 3px;
        top: 10px;
        bottom: 10px;
        width: 3px;
        border-radius: 999px;
        background: var(--color-accent);
    }

    .ch-row-check {
        margin-top: 2px;
        flex-shrink: 0;
        accent-color: var(--color-accent);
    }
    .ch-row-main {
        flex: 1;
        min-width: 0;
        display: flex;
        flex-direction: column;
        gap: 6px;
    }
    .ch-row-meta {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 6px;
        font-size: 11px;
        color: var(--color-muted);
    }
    .ch-meta-dot {
        opacity: 0.6;
    }
    .ch-cat-pill {
        display: inline-flex;
        align-items: center;
        gap: 4px;
        height: 18px;
        padding: 0 7px;
        border-radius: var(--radius-pill);
        background: var(--color-panel);
        border: 1px solid var(--color-divider, var(--color-border));
        color: var(--color-text-secondary);
        font-size: 10px;
        font-weight: 500;
    }
    :global(.ch-cat-pill-ico) {
        width: 10px;
        height: 10px;
    }
    .ch-pinned-tag {
        color: var(--color-accent);
        font-weight: 500;
    }
    .ch-source {
        display: inline-flex;
        align-items: center;
        gap: 5px;
    }
    .ch-source-dot {
        display: inline-block;
        width: 6px;
        height: 6px;
        border-radius: 50%;
        flex-shrink: 0;
        box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.35);
    }
    .ch-sensitive {
        display: inline-flex;
        align-items: center;
        gap: 4px;
        height: 18px;
        padding: 0 7px;
        border-radius: var(--radius-pill);
        background: var(--color-warning-soft);
        border: 1px solid var(--color-warning-strong);
        color: var(--color-warning);
        font-size: 10px;
        font-weight: 600;
    }
    :global(.ch-sensitive-ico) {
        width: 10px;
        height: 10px;
    }

    /* ─── Pinned label editor ──────────────────────────────── */
    .ch-label-row {
        display: flex;
        align-items: center;
        gap: 6px;
    }
    .ch-label-input {
        flex: 1;
        height: 28px;
        padding: 0 10px;
        background: var(--color-panel);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        color: var(--color-text);
        font-size: 12px;
        outline: none;
        transition: border-color var(--dur-micro) var(--ease-out);
    }
    .ch-label-input:focus {
        border-color: var(--color-accent);
    }
    .ch-label-chip {
        display: inline-flex;
        align-items: center;
        gap: 5px;
        align-self: flex-start;
        height: 22px;
        padding: 0 8px;
        border-radius: var(--radius-pill);
        border: 1px solid color-mix(in srgb, var(--color-accent) 30%, var(--color-border));
        background: var(--color-accent-soft);
        color: var(--color-accent);
        font-size: 11.5px;
        font-weight: 500;
        cursor: pointer;
        transition: background-color var(--dur-micro) var(--ease-out);
    }
    .ch-label-chip:hover {
        background: color-mix(in srgb, var(--color-accent) 18%, transparent);
    }
    :global(.ch-label-chip-ico) {
        width: 11px;
        height: 11px;
    }
    .ch-label-add {
        display: inline-flex;
        align-items: center;
        gap: 4px;
        align-self: flex-start;
        background: transparent;
        border: none;
        padding: 0;
        color: var(--color-muted);
        font-size: 10.5px;
        cursor: pointer;
        transition: color var(--dur-micro) var(--ease-out);
    }
    .ch-label-add:hover {
        color: var(--color-accent);
    }
    :global(.ch-label-add-ico) {
        width: 10px;
        height: 10px;
    }

    /* ─── Image block inside a row ──────────────────────────── */
    .ch-image-row {
        display: flex;
        align-items: flex-start;
        gap: 12px;
    }
    .ch-image-meta {
        font-size: 11px;
        line-height: 1.55;
        color: var(--color-muted);
    }
    .ch-image-fmt-row {
        display: flex;
        align-items: center;
        gap: 6px;
        margin-bottom: 2px;
    }
    .ch-image-fmt {
        padding: 1px 6px;
        background: var(--color-panel);
        border: 1px solid var(--color-border);
        border-radius: 4px;
        font-size: 10px;
        font-weight: 600;
        text-transform: uppercase;
        letter-spacing: 0.05em;
        color: var(--color-text);
    }
    .ch-image-animated {
        padding: 1px 6px;
        background: var(--color-accent-soft);
        border: 1px solid color-mix(in srgb, var(--color-accent) 30%, var(--color-border));
        border-radius: var(--radius-pill);
        font-size: 10px;
        color: var(--color-accent);
    }

    /* ─── Body text preview ─────────────────────────────────── */
    .ch-text {
        margin: 0;
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 11.5px;
        line-height: 1.55;
        color: var(--color-text);
        white-space: pre-wrap;
        word-break: break-word;
    }

    /* Color row — swatch + value inline. Inset rings on the swatch
       keep light or dark colors defined against the row background.
       Same recipe as the command-palette color swatch. */
    .ch-color-row {
        display: flex;
        align-items: center;
        gap: 10px;
    }
    .ch-color-swatch {
        flex: none;
        width: 24px;
        height: 24px;
        border-radius: 6px;
        box-shadow:
            inset 0 0 0 1px rgba(0, 0, 0, 0.30),
            inset 0 0 0 2px rgba(255, 255, 255, 0.08);
    }
    .ch-text-color {
        font-size: 13px;
        font-weight: 500;
        letter-spacing: 0.005em;
    }

    /* ─── Hover-revealed row actions ────────────────────────── */
    .ch-row-actions {
        flex: none;
        display: flex;
        align-items: center;
        gap: 4px;
        opacity: 0.5;
        transition: opacity var(--dur-micro) var(--ease-out);
    }
    .ch-row:hover .ch-row-actions,
    .ch-row:focus-within .ch-row-actions {
        opacity: 1;
    }

    /* ─── Empty / sentinel ──────────────────────────────────── */
    .ch-empty {
        padding: 28px 16px;
        background: var(--color-panel-2);
        border: 1px dashed var(--color-divider, var(--color-border));
        border-radius: var(--radius-control);
        text-align: center;
    }
    .ch-empty-text {
        margin: 0;
        font-size: 12.5px;
        color: var(--color-text-secondary);
    }
    .ch-sentinel {
        display: flex;
        align-items: center;
        justify-content: center;
        gap: 8px;
        padding: 12px;
        background: var(--color-panel-2);
        border: 1px dashed var(--color-divider, var(--color-border));
        border-radius: var(--radius-control);
        font-size: 11px;
        color: var(--color-muted);
    }
    .ch-sentinel-spin {
        display: inline-block;
        width: 11px;
        height: 11px;
        border-radius: 50%;
        border: 2px solid var(--color-accent-soft);
        border-top-color: var(--color-accent);
        animation: ch-spin 760ms linear infinite;
    }
    @keyframes ch-spin {
        to {
            transform: rotate(360deg);
        }
    }

    /* ─── Footer keyboard hint ──────────────────────────────── */
    .ch-footer-line {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        flex-wrap: wrap;
    }
    :global(.ch-footer-ico) {
        width: 13px;
        height: 13px;
        color: var(--color-accent);
    }

    /* ─── SideSheet content ─────────────────────────────────── */
    .ch-sheet-stack {
        display: flex;
        flex-direction: column;
        gap: 16px;
    }
    .ch-sheet-desc {
        margin: 0;
        font-size: 12.5px;
        line-height: 1.55;
        color: var(--color-text-secondary);
    }
    .ch-sheet-field {
        display: flex;
        flex-direction: column;
        gap: 8px;
    }
    .ch-sheet-label-row {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
    }
    .ch-sheet-label {
        display: block;
        font-size: 11px;
        font-weight: 600;
        text-transform: uppercase;
        letter-spacing: 0.06em;
        color: var(--color-muted);
    }
    .ch-sheet-count {
        font-size: 11px;
        color: var(--color-muted);
        font-variant-numeric: tabular-nums;
    }
    .ch-sheet-row {
        display: flex;
        align-items: center;
        gap: 8px;
        flex-wrap: wrap;
    }
    .ch-sheet-input {
        flex: 1;
        min-width: 0;
        height: 34px;
        padding: 0 11px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        color: var(--color-text);
        font-size: 13px;
        outline: none;
        transition: border-color var(--dur-micro) var(--ease-out);
    }
    .ch-sheet-input:focus {
        border-color: var(--color-accent);
    }
    .ch-sheet-input-num {
        flex: none;
        width: 84px;
        text-align: right;
    }
    .ch-sheet-hint {
        font-size: 11.5px;
        color: var(--color-muted);
    }
    .ch-sheet-toggle {
        display: flex;
        align-items: flex-start;
        justify-content: space-between;
        gap: 12px;
        padding: 10px 12px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-divider, var(--color-border));
        border-radius: var(--radius-control);
        cursor: pointer;
    }
    .ch-sheet-toggle-text {
        flex: 1;
        min-width: 0;
        display: flex;
        flex-direction: column;
        gap: 3px;
    }
    .ch-sheet-toggle-label {
        font-size: 13px;
        font-weight: 500;
        color: var(--color-text);
    }
    .ch-sheet-toggle-hint {
        font-size: 11.5px;
        line-height: 1.45;
        color: var(--color-text-secondary);
    }
    .ch-sheet-checkbox {
        margin-top: 3px;
        flex-shrink: 0;
        accent-color: var(--color-accent);
    }
    .ch-sheet-divider {
        height: 1px;
        background: var(--color-divider, var(--color-border));
    }
    .ch-sheet-empty {
        margin: 0;
        padding: 14px 12px;
        background: var(--color-panel-2);
        border: 1px dashed var(--color-divider, var(--color-border));
        border-radius: var(--radius-control);
        font-size: 12px;
        color: var(--color-muted);
        text-align: center;
    }

    /* ─── Exclusion tags ─────────────────────────────────────── */
    .ch-excl-tags {
        display: flex;
        flex-wrap: wrap;
        gap: 6px;
    }
    .ch-excl-tag {
        display: inline-flex;
        align-items: center;
        gap: 4px;
        height: 26px;
        padding: 0 4px 0 10px;
        border-radius: var(--radius-pill);
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        color: var(--color-text);
        font-size: 12px;
    }
    .ch-excl-remove {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 18px;
        height: 18px;
        background: transparent;
        border: none;
        border-radius: 50%;
        color: var(--color-muted);
        cursor: pointer;
        transition:
            background-color var(--dur-micro) var(--ease-out),
            color var(--dur-micro) var(--ease-out);
    }
    .ch-excl-remove:hover {
        background: var(--color-error-soft);
        color: var(--color-error);
    }
    :global(.ch-excl-remove-ico) {
        width: 11px;
        height: 11px;
    }

    /* ─── Snippets sheet body ───────────────────────────────── */
    .ch-snippets-empty {
        display: flex;
        align-items: center;
        justify-content: center;
        gap: 8px;
        padding: 14px 12px;
        background: var(--color-panel-2);
        border: 1px dashed var(--color-divider, var(--color-border));
        border-radius: var(--radius-control);
        color: var(--color-muted);
        font-size: 12.5px;
        cursor: pointer;
        transition:
            border-color var(--dur-micro) var(--ease-out),
            color var(--dur-micro) var(--ease-out);
    }
    .ch-snippets-empty:hover {
        border-color: color-mix(in srgb, var(--color-accent) 50%, var(--color-border));
        color: var(--color-accent);
    }
    :global(.ch-snippets-empty-ico) {
        width: 13px;
        height: 13px;
    }
    .ch-snippets-list {
        display: flex;
        flex-direction: column;
        gap: 6px;
    }
    .ch-snippets-row {
        display: flex;
        align-items: center;
        gap: 10px;
        padding: 10px 12px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        text-align: left;
        cursor: pointer;
        transition: border-color var(--dur-micro) var(--ease-out);
    }
    .ch-snippets-row:hover {
        border-color: color-mix(in srgb, var(--color-accent) 50%, var(--color-border));
    }
    :global(.ch-snippets-row-ico) {
        width: 13px;
        height: 13px;
        flex-shrink: 0;
        color: var(--color-accent);
    }
    .ch-snippets-row-text {
        flex: 1;
        min-width: 0;
        display: flex;
        flex-direction: column;
        gap: 2px;
    }
    .ch-snippets-row-label {
        font-size: 12.5px;
        color: var(--color-text);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .ch-snippets-row-trigger {
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 11px;
        color: var(--color-accent);
    }

    /* ─── Thumbnail (image rows) ────────────────────────────── */
    .thumb-button {
        all: unset;
        position: relative;
        display: inline-flex;
        cursor: pointer;
        border-radius: 6px;
        outline: none;
    }
    .image-thumb {
        max-width: 200px;
        max-height: 140px;
        border-radius: 6px;
        border: 1px solid var(--color-border);
        background: transparent;
        object-fit: contain;
        transition: transform 160ms ease, border-color 160ms ease, box-shadow 160ms ease;
    }
    .thumb-button:hover .image-thumb {
        transform: scale(1.02);
        border-color: color-mix(in srgb, var(--color-accent) 55%, var(--color-border));
        box-shadow: 0 8px 22px rgba(0, 0, 0, 0.35);
    }
    .thumb-button:focus-visible .image-thumb {
        outline: 2px solid var(--color-accent);
        outline-offset: 2px;
    }
    .thumb-zoom-badge {
        position: absolute;
        right: 6px;
        bottom: 6px;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 26px;
        height: 26px;
        border-radius: 999px;
        background: rgba(0, 0, 0, 0.62);
        color: #fff;
        border: 1px solid rgba(255, 255, 255, 0.12);
        opacity: 0;
        transform: translateY(2px) scale(0.92);
        transition: opacity 140ms ease, transform 140ms ease;
        pointer-events: none;
    }
    :global(.thumb-zoom-ico) {
        width: 13px;
        height: 13px;
    }
    .thumb-button:hover .thumb-zoom-badge,
    .thumb-button:focus-visible .thumb-zoom-badge {
        opacity: 1;
        transform: translateY(0) scale(1);
    }

    /* ─── History scrollbar styling ────────────────────────── */
    .history-scroll {
        scrollbar-width: thin;
        scrollbar-color: rgba(255, 255, 255, 0.12) transparent;
    }
    .history-scroll::-webkit-scrollbar {
        width: 8px;
    }
    .history-scroll::-webkit-scrollbar-thumb {
        background: rgba(255, 255, 255, 0.08);
        border-radius: 4px;
    }
    .history-scroll::-webkit-scrollbar-thumb:hover {
        background: rgba(255, 255, 255, 0.18);
    }

    /* ─── Image preview modal ───────────────────────────────── */
    .ch-preview-backdrop {
        position: fixed;
        inset: 0;
        z-index: 90;
        display: flex;
        align-items: center;
        justify-content: center;
        padding: 32px;
        background: rgba(0, 0, 0, 0.78);
        animation: ch-preview-fade-in 180ms ease;
    }
    @keyframes ch-preview-fade-in {
        from {
            opacity: 0;
        }
        to {
            opacity: 1;
        }
    }
    .ch-preview-stage {
        position: relative;
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 14px;
        max-width: min(96vw, 1200px);
        max-height: 92vh;
    }
    .ch-preview-image {
        max-width: 100%;
        max-height: calc(92vh - 70px);
        object-fit: contain;
        border-radius: 12px;
        background: transparent;
        border: 1px solid rgba(255, 255, 255, 0.08);
        box-shadow: 0 14px 38px rgba(0, 0, 0, 0.55);
        animation: ch-preview-pop 220ms cubic-bezier(0.2, 0.9, 0.3, 1);
    }
    @keyframes ch-preview-pop {
        from {
            opacity: 0;
            transform: scale(0.95);
        }
        to {
            opacity: 1;
            transform: scale(1);
        }
    }
    .ch-preview-meta {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        justify-content: center;
        gap: 8px;
        font-size: 11px;
        color: var(--color-text-secondary);
    }
    .ch-preview-tag {
        padding: 2px 8px;
        border-radius: var(--radius-pill);
        border: 1px solid var(--color-border);
        background: var(--color-panel-2);
        font-weight: 600;
        font-size: 10px;
        text-transform: uppercase;
        letter-spacing: 0.05em;
        color: var(--color-text);
    }
    .ch-preview-tag-accent {
        border-color: color-mix(in srgb, var(--color-accent) 55%, var(--color-border));
        background: var(--color-accent-soft);
        color: var(--color-accent);
    }
    .ch-preview-dim {
        color: var(--color-muted);
    }
    .ch-preview-hint {
        margin-left: 4px;
        color: var(--color-muted);
        font-style: italic;
    }
    .ch-preview-close {
        position: absolute;
        top: -42px;
        right: 0;
        width: 32px;
        height: 32px;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        border-radius: 999px;
        border: 1px solid rgba(255, 255, 255, 0.12);
        background: rgba(255, 255, 255, 0.06);
        color: #f3f3f3;
        cursor: pointer;
        transition: background-color 140ms ease, border-color 140ms ease;
    }
    .ch-preview-close:hover {
        background: rgba(255, 255, 255, 0.14);
        border-color: rgba(255, 255, 255, 0.24);
    }
    :global(.ch-preview-close-ico) {
        width: 15px;
        height: 15px;
    }
</style>
