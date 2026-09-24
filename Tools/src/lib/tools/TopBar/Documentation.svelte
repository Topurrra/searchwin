<script lang="ts">
    /** Task-oriented documentation for visible, shipped workflows. */
    import { onMount } from 'svelte';
    import { _ } from 'svelte-i18n';
    import { ToolPage } from '$lib/ui';
    import { escToClear } from '$lib/actions/escToClear';
    import {
        ArrowRight,
        BookOpen,
        Search as SearchIcon,
        Files,
        Clock3,
        Image as ImageIcon,
        Video,
        Clipboard,
        ShieldCheck,
        Code as CodeIcon,
        Mic,
        Wand2,
        ChevronDown,
        AlertTriangle,
        X,
    } from '@lucide/svelte';
    import { toolScreens, toolPackIdForScreen } from '$lib/appScreens';
    import { enabledPackIds } from '$lib/stores/toolPacks';
    import { settingsTarget } from '$lib/stores/settingsTarget';
    import { toast } from '$lib/stores/toasts';

    let { selected = $bindable() }: { selected: string } = $props();

    /** Is the tool behind a how-to card in an ENABLED tool pack? Pass the live
     *  `$enabledPackIds` so the template stays reactive as packs toggle. */
    function isToolPackEnabled(toolId: string, enabled: string[]): boolean {
        const screen = toolScreens.find((s) => s.id === toolId);
        return screen ? enabled.includes(toolPackIdForScreen(screen)) : true;
    }

    /** Open the card's tool — but if its pack is disabled, route to Settings →
     *  Tool Packs with a hint instead of opening a tool that isn't installed. */
    function openHowToTool(toolId: string, toolLabel: string, packEnabled: boolean) {
        if (packEnabled) {
            selected = toolId;
            return;
        }
        settingsTarget.set('toolpacks');
        toast(`${toolLabel} is in a tool pack that isn't enabled — turn it on in Settings.`, 'info');
        selected = 'settings';
    }

    /** Each how-to maps an intent to a target tool + practical playbook.
     * Keep `title` phrased as the user would say it to themselves. */
    type HowTo = {
        id: string;
        category: CategoryId;
        title: string;
        scenario: string;
        toolId: string;
        toolLabel: string;
        steps: string[];
        gotchas?: string[];
    };

    type CategoryId =
        | 'find'
        | 'image'
        | 'media'
        | 'document'
        | 'clipboard'
        | 'voice'
        | 'privacy'
        | 'dev'
        | 'generate'
        | 'work';

    type Category = { id: CategoryId | 'all'; label: string; icon: any };

    const CATEGORIES: Category[] = [
        { id: 'all', label: 'All', icon: BookOpen },
        { id: 'find', label: 'Find & organize', icon: SearchIcon },
        { id: 'image', label: 'Images & scans', icon: ImageIcon },
        { id: 'media', label: 'Media', icon: Video },
        { id: 'document', label: 'Documents & notes', icon: Files },
        { id: 'clipboard', label: 'Clipboard & commands', icon: Clipboard },
        { id: 'voice', label: 'Voice', icon: Mic },
        { id: 'privacy', label: 'Privacy', icon: ShieldCheck },
        { id: 'dev', label: 'Developer tools', icon: CodeIcon },
        { id: 'generate', label: 'Utilities & generate', icon: Wand2 },
        { id: 'work', label: 'Workspaces', icon: Clock3 },
    ];

    const EXTRA_HOW_TOS: HowTo[] = [
        {
            id: 'command-search',
            category: 'find',
            title: 'Find local files quickly',
            scenario: 'You remember part of a file name, path, or phrase but not where it lives.',
            toolId: 'command',
            toolLabel: 'Command',
            steps: [
                'Open Command and keep Search selected.',
                'Type part of the name, path, or indexed content.',
                'Open the result you need or narrow the query with a filter.',
            ],
            gotchas: ['Choose folders in File Search Index and build the index before expecting local results.'],
        },
        {
            id: 'find-duplicates',
            category: 'find',
            title: 'Find duplicate files',
            scenario: 'The same photos, installers, or documents may exist in several folders.',
            toolId: 'duplicate-finder',
            toolLabel: 'Duplicate Finder',
            steps: [
                'Choose the folders to scan.',
                'Run the scan to group files with matching contents.',
                'Review each group and move selected copies to review before deleting anything.',
            ],
        },
        {
            id: 'markdown-workflow',
            category: 'document',
            title: 'Preview Markdown or turn HTML back into Markdown',
            scenario: 'You want to read, clean up, or convert document text locally.',
            toolId: 'markdown-converter',
            toolLabel: 'Markdown Converter',
            steps: [
                'Write, paste, or open a Markdown file.',
                'Use the live preview to inspect headings, lists, tables, and links.',
                'Switch to the HTML-to-Markdown flow when you need clean Markdown from HTML.',
            ],
        },
        {
            id: 'notes-workflow',
            category: 'document',
            title: 'Keep connected local notes',
            scenario: 'You want editable notes that stay as files on your own machine.',
            toolId: 'notes',
            toolLabel: 'Notes',
            steps: [
                'Create a note or start from a template.',
                'Write, tag, and link notes while the editor saves locally.',
                'Use the library, outline, backlinks, or split view to return to related context.',
            ],
        },
        {
            id: 'clipboard-workflow',
            category: 'clipboard',
            title: 'Recall something you copied earlier',
            scenario: 'A useful link, message, or code snippet is no longer on your clipboard.',
            toolId: 'clipboard-history',
            toolLabel: 'Clipboard History',
            steps: [
                'Open Command and switch to Clipboard.',
                'Search the local history or filter the entries.',
                'Copy or paste the item you need.',
            ],
        },
        {
            id: 'snippets-workflow',
            category: 'clipboard',
            title: 'Reuse a text template',
            scenario: 'You repeatedly write the same response, status update, or boilerplate.',
            toolId: 'snippets',
            toolLabel: 'Snippets',
            steps: [
                'Open Command, choose Clipboard, then open Snippets.',
                'Create a short trigger and template body with the variables you need.',
                'Use the clipboard overlay to find the trigger and paste the expanded template.',
            ],
        },
        {
            id: 'commands-workflow',
            category: 'clipboard',
            title: 'Create a personal command shortcut',
            scenario: 'You want one keyword to open a URL, app, file, folder, or trusted shell command.',
            toolId: 'my-commands',
            toolLabel: 'My Commands',
            steps: [
                'Open My Commands and add a new command.',
                'Choose the action type and give it a clear keyword.',
                'Run that keyword from the command palette whenever you need it.',
            ],
        },
        {
            id: 'voice-workflow',
            category: 'voice',
            title: 'Dictate text locally',
            scenario: 'You want a short transcription or a continuous dictation session without a cloud service.',
            toolId: 'voice-to-text',
            toolLabel: 'Voice to Text',
            steps: [
                'Open Command and switch to Voice.',
                'Install or choose a language pack in Settings if the page asks for one.',
                'Start a one-shot or continuous session, then copy the transcript you need.',
            ],
        },
        {
            id: 'privacy-audit-workflow',
            category: 'privacy',
            title: 'Check your local privacy posture',
            scenario: 'You want a deliberate review of active permissions, startup items, extensions, and exposed secrets.',
            toolId: 'privacy-audit',
            toolLabel: 'Privacy Audit',
            steps: [
                'Open Privacy Audit and run a scan when you are ready.',
                'Review the active findings and their severity.',
                'Use the provided setting or file action to investigate or fix a finding.',
            ],
            gotchas: ['The audit is user-initiated; it does not scan in the background.'],
        },
        {
            id: 'time-tracker-workflow',
            category: 'work',
            title: 'Understand where your work time goes',
            scenario: 'You want a private view of activity, categories, and the apps that filled your day.',
            toolId: 'time-tracker',
            toolLabel: 'Time Tracker',
            steps: [
                'Start tracking when you are ready to build a local activity journal.',
                'Use the activity flow or daily rhythm to inspect recorded blocks.',
                'Adjust categories, rules, goals, or excluded apps to keep the journal useful.',
            ],
        },
        {
            id: 'screen-recording-workflow',
            category: 'media',
            title: 'Record your screen locally',
            scenario: 'You need a clean MP4 of your primary screen or a selected region.',
            toolId: 'screen-recorder',
            toolLabel: 'Screen Recorder',
            steps: [
                'Locate a compatible local ffmpeg.exe if the setup card appears.',
                'Choose the full screen or select a region.',
                'Configure audio and any redactions or excluded windows you need.',
                'Start recording, then stop and save the local MP4 when you are done.',
            ],
            gotchas: [
                'KeepItLocal does not bundle FFmpeg. Screen recording needs a local FFmpeg build with the Windows h264_mf encoder.',
                'Redaction regions and excluded windows keep those pixels out of the recording.',
            ],
        },
        {
            id: 'media-compression-workflow',
            category: 'media',
            title: 'Shrink a video or extract its audio',
            scenario: 'You need a smaller local video to share, or just its MP3 audio track.',
            toolId: 'media-utility',
            toolLabel: 'Media Utility',
            steps: [
                'Locate a full local FFmpeg build if Media Utility asks for it, then choose a video.',
                'Choose Extract MP3 for audio only, or Compress for a smaller video.',
                'Pick a size preset or enter a positive custom MB target, then run the job.',
            ],
            gotchas: [
                'MP3 extraction needs FFmpeg. Video compression also needs ffprobe and the Windows h264_mf encoder.',
                'The source timeline shows media position, not the remaining wall-clock time.',
                'You can navigate away while a job runs. Return to Media Utility to see its live progress or cancel it.',
            ],
        },
        {
            id: 'archive-workflow',
            category: 'find',
            title: 'Create or extract a local archive',
            scenario: 'You need to package files, inspect an archive, or unpack one without an online service.',
            toolId: 'archive-utility',
            toolLabel: 'Archive Utility',
            steps: [
                'Choose Create to add files, select ZIP, 7Z, TAR.ZST, TAR.GZ, or TAR.XZ, and choose an output name.',
                'Use ZIP for broad compatibility, 7Z for compact folders, or TAR.ZST for fast large backups.',
                'Choose Inspect or Extract for an existing archive; add a password for protected ZIP or 7Z archives.',
            ],
            gotchas: [
                'Cancel stops archive creation without promoting the partial archive. Files already extracted remain in the chosen folder.',
            ],
        },
    ];

    const CATEGORY_BY_TOOL: Partial<Record<string, CategoryId>> = {
        File: 'find',
        Image: 'image',
        Media: 'media',
        Document: 'document',
        Privacy: 'privacy',
        Development: 'dev',
        Utils: 'generate',
        Focus: 'work',
    };

    const HOW_TOS: HowTo[] = [
        ...EXTRA_HOW_TOS,
        ...toolScreens.flatMap((screen) => {
            const category = CATEGORY_BY_TOOL[screen.category];
            if (
                !category ||
                screen.hidden ||
                !screen.docs ||
                screen.id === 'markdown-converter' ||
                screen.id === 'media-utility' ||
                screen.id === 'archive-utility'
            )
                return [];
            return [
                {
                    id: `tool-${screen.id}`,
                    category,
                    title: screen.name,
                    scenario: screen.description,
                    toolId: screen.id,
                    toolLabel: screen.name,
                    steps: [`Open ${screen.name}.`, screen.docs.use],
                    gotchas: [screen.docs.tip],
                },
            ];
        }),
    ];

    let query = $state('');
    let activeCategory = $state<CategoryId | 'all'>('all');
    let expandedIds = $state(new Set<string>());

    let filteredHowTos = $derived.by(() => {
        const q = query.trim().toLowerCase();
        return HOW_TOS.filter((howto) => {
            if (activeCategory !== 'all' && howto.category !== activeCategory) return false;
            if (!q) return true;
            const haystack = `${howto.title} ${howto.scenario} ${howto.toolLabel} ${howto.steps.join(' ')} ${(howto.gotchas ?? []).join(' ')}`.toLowerCase();
            return haystack.includes(q);
        });
    });

    let categoryCounts = $derived.by(() => {
        const counts: Record<string, number> = { all: HOW_TOS.length };
        for (const cat of CATEGORIES) {
            if (cat.id === 'all') continue;
            counts[cat.id] = HOW_TOS.filter((h) => h.category === cat.id).length;
        }
        return counts;
    });

    function toggle(id: string) {
        const next = new Set(expandedIds);
        if (next.has(id)) next.delete(id);
        else next.add(id);
        expandedIds = next;
    }

    function expandAll() {
        expandedIds = new Set(filteredHowTos.map((h) => h.id));
    }
    function collapseAll() {
        expandedIds = new Set();
    }

    /** Focus the search input via `/` shortcut. Same UX as Privacy Guide. */
    let searchInput: HTMLInputElement | null = $state(null);
    function onPageKeydown(event: KeyboardEvent) {
        if (event.key !== '/') return;
        const target = event.target as HTMLElement | null;
        if (target && (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable)) return;
        if (searchInput) {
            event.preventDefault();
            searchInput.focus();
            searchInput.select();
        }
    }

    onMount(() => {
        document.addEventListener('keydown', onPageKeydown);
        return () => document.removeEventListener('keydown', onPageKeydown);
    });
</script>

<ToolPage
    icon={BookOpen}
    iconTint="var(--color-accent)"
    title={$_('page.docs.heroTitle')}
    description={$_('page.docs.heroSubtitle')}
    width="wide"
    fill={false}
>
    <!-- Search + filter chips -->
    <div class="rounded-2xl border border-border bg-panel p-3 md:p-4 space-y-3">
        <div class="relative">
            <span class="pointer-events-none absolute left-2 top-1/2 inline-flex h-7 w-7 -translate-y-1/2 items-center justify-center rounded-md border border-accent/25 bg-accent/10 text-accent">
                <SearchIcon class="h-3.5 w-3.5" />
            </span>
            <input
                bind:this={searchInput}
                bind:value={query}
                use:escToClear={() => (query = '')}
                onkeydown={(e) => {
                    if (e.key === 'Escape' && query) {
                        e.preventDefault();
                        query = '';
                    }
                }}
                placeholder="Search how-tos..."
                class="h-10 w-full rounded-lg border border-border bg-panel-2 pl-12 pr-20 text-sm outline-none transition-colors focus:border-accent"
            />
            {#if query}
                <button
                    type="button"
                    onclick={() => (query = '')}
                    class="absolute right-2 top-1/2 -translate-y-1/2 inline-flex h-6 items-center gap-1 rounded border border-border bg-panel px-2 text-[10px] text-muted hover:border-accent/60 hover:text-text transition-colors"
                    aria-label={$_('page.docs.clearSearch')}
                >
                    <X class="h-3 w-3" />
                    {$_('page.docs.clear')}
                </button>
            {:else}
                <kbd class="absolute right-2 top-1/2 -translate-y-1/2 rounded border border-border bg-panel px-1.5 py-0.5 text-[10px] font-mono text-muted">/</kbd>
            {/if}
        </div>

        <div class="flex flex-wrap items-center gap-2">
            {#each CATEGORIES as cat (cat.id)}
                {@const Icon = cat.icon}
                {@const count = categoryCounts[cat.id] ?? 0}
                {@const active = activeCategory === cat.id}
                <button
                    type="button"
                    onclick={() => (activeCategory = cat.id as CategoryId | 'all')}
                    class="inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-xs transition-colors
                        {active
                        ? 'border-accent bg-accent/15 text-accent'
                        : 'border-border bg-panel-2 text-muted hover:border-accent/40 hover:text-text'}"
                >
                    <Icon class="h-3 w-3" />
                    {cat.label}
                    <span class="rounded-full bg-panel px-1.5 py-0.5 text-[10px]">{count}</span>
                </button>
            {/each}

            <div class="ml-auto flex items-center gap-2 text-xs text-muted">
                <button
                    type="button"
                    onclick={expandAll}
                    class="hover:text-text transition-colors"
                >{$_('page.docs.expandAll')}</button>
                <span>·</span>
                <button
                    type="button"
                    onclick={collapseAll}
                    class="hover:text-text transition-colors"
                >{$_('page.docs.collapseAll')}</button>
            </div>
        </div>
    </div>

    <!-- How-to cards -->
    {#if filteredHowTos.length === 0}
        <div class="rounded-xl border border-border border-dashed bg-panel-2/40 p-8 text-center text-sm text-muted">
            {#if query.trim()}
                {$_('page.docs.noMatchPrefix')} <strong class="text-text">"{query}"</strong>.
            {:else}
                {$_('page.docs.noneInCategory')}
            {/if}
        </div>
    {:else}
        <div class="space-y-2">
            {#each filteredHowTos as howto (howto.id)}
                {@const expanded = expandedIds.has(howto.id)}
                <article class="rounded-xl border border-border bg-panel transition-colors hover:border-accent/40">
                    <button
                        type="button"
                        onclick={() => toggle(howto.id)}
                        class="w-full p-4 text-left flex items-start justify-between gap-3"
                    >
                        <div class="min-w-0 flex-1">
                            <div class="flex items-center gap-2 mb-1">
                                <span class="text-sm font-semibold text-text">{howto.title}</span>
                                <span class="inline-block rounded-md border border-border bg-panel-2 px-1.5 py-0.5 text-[10px] text-muted">
                                    {howto.toolLabel}
                                </span>
                            </div>
                            <div class="text-xs leading-relaxed text-muted">{howto.scenario}</div>
                        </div>
                        <ChevronDown class="h-4 w-4 text-muted shrink-0 transition-transform {expanded ? 'rotate-180' : ''}" />
                    </button>

                    {#if expanded}
                        <div class="border-t border-border p-4 space-y-3">
                            <div>
                                <div class="text-xs uppercase tracking-wider text-muted font-semibold mb-2">{$_('page.docs.steps')}</div>
                                <ol class="space-y-1.5">
                                    {#each howto.steps as step, i}
                                        <li class="flex items-start gap-2 text-xs leading-relaxed text-text">
                                            <span class="flex h-4 w-4 shrink-0 items-center justify-center rounded-full bg-accent/15 text-[9px] font-semibold text-accent mt-0.5">
                                                {i + 1}
                                            </span>
                                            <span>{step}</span>
                                        </li>
                                    {/each}
                                </ol>
                            </div>

                            {#if howto.gotchas && howto.gotchas.length > 0}
                                <div>
                                    <div class="text-xs uppercase tracking-wider text-muted font-semibold mb-2 flex items-center gap-1.5">
                                        <AlertTriangle class="h-3 w-3 text-warning" />
                                        {$_('page.docs.commonGotchas')}
                                    </div>
                                    <ul class="space-y-1">
                                        {#each howto.gotchas as gotcha}
                                            <li class="text-xs leading-relaxed text-text-secondary pl-6 -indent-3">
                                                <span class="text-warning mr-2">•</span>{gotcha}
                                            </li>
                                        {/each}
                                    </ul>
                                </div>
                            {/if}

                            <div class="pt-2 border-t border-border">
                                <button
                                    type="button"
                                    onclick={() =>
                                        openHowToTool(
                                            howto.toolId,
                                            howto.toolLabel,
                                            isToolPackEnabled(howto.toolId, $enabledPackIds),
                                        )}
                                    class="inline-flex items-center gap-1.5 rounded-lg bg-accent px-3 py-1.5 text-xs font-medium text-accent-contrast hover:bg-accent-hover {isToolPackEnabled(
                                        howto.toolId,
                                        $enabledPackIds,
                                    )
                                        ? ''
                                        : 'opacity-60'}"
                                    title={isToolPackEnabled(howto.toolId, $enabledPackIds)
                                        ? ''
                                        : `${howto.toolLabel} is in a disabled tool pack — enable it in Settings`}
                                >
                                    {isToolPackEnabled(howto.toolId, $enabledPackIds)
                                        ? $_('page.docs.open', { values: { tool: howto.toolLabel } })
                                        : 'Enable in Settings'}
                                    <ArrowRight class="h-3 w-3" />
                                </button>
                            </div>
                        </div>
                    {/if}
                </article>
            {/each}
        </div>
    {/if}
</ToolPage>
