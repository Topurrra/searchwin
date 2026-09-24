<script lang="ts">
    import { onMount } from 'svelte';
    import { _ } from 'svelte-i18n';
    import {
        ArrowLeft,
        BookOpen,
        CheckCircle2,
        ShieldCheck,
        Search,
        Lock,
        KeyRound,
        Smartphone,
        Wifi,
        Globe,
        HardDrive,
        AlertTriangle,
        EyeOff,
        RotateCcw,
        Lightbulb,
        FileText,
        Mail,
        Mic,
        CreditCard,
        MessageSquareLock,
        Download,
        Trash2,
        Users,
        Camera,
        BrainCircuit,
        Settings2,
        Info,
    } from '@lucide/svelte';
    import { ToolPage } from '$lib/ui';
    import { escToClear } from '$lib/actions/escToClear';

    type Level = 'Basic' | 'Smart' | 'Careful';
    type Tab = 'learn' | 'myths' | 'privacy';

    type Topic = {
        id: string;
        title: string;
        short: string;
        level: Level;
        minutes: number;
        icon: typeof Lock;
        tags: string[];
        takeaway: string;
        doNow: string[];
        avoid: string[];
        details: string[];
        scenarios?: string[];
        examples?: string[];
        oneClickFixes?: string[];
        selfChecks?: string[];
    };

    type ArticleSection = {
        heading: string;
        points: string[];
    };

    type Myth = {
        myth: string;
        reality: string;
    };

    const DENSITY_KEY = 'keepitlocal_privacy_guide_compact_v1';

    /** Static, locale-independent metadata for each guide topic. The
     * prose (title, short, takeaway, doNow, avoid, details, …) lives in
     * the locale bundle under `page.privacyGuide.topics.<id>.*` and is
     * resolved at render via `resolveTopic`. `level` stays the English
     * literal because `levelClass`/`levelLabel` switch on it; the array
     * counts let us map index → key for the per-item string lists. */
    type TopicSpec = {
        id: string;
        level: Level;
        minutes: number;
        icon: typeof Lock;
        tags: number;
        doNow: number;
        avoid: number;
        details: number;
        scenarios?: number;
        examples?: number;
        oneClickFixes?: number;
        selfChecks?: number;
    };

    const topicSpecs: TopicSpec[] = [
        { id: 'keepitlocal-data', level: 'Basic', minutes: 4, icon: ShieldCheck, tags: 5, doNow: 4, avoid: 3, details: 5, scenarios: 4, examples: 3, oneClickFixes: 3, selfChecks: 3 },
        { id: 'passwords', level: 'Basic', minutes: 5, icon: KeyRound, tags: 3, doNow: 4, avoid: 4, details: 3 },
        { id: 'mfa-passkeys', level: 'Basic', minutes: 6, icon: ShieldCheck, tags: 4, doNow: 4, avoid: 4, details: 3 },
        { id: 'phishing', level: 'Basic', minutes: 7, icon: AlertTriangle, tags: 4, doNow: 4, avoid: 4, details: 3 },
        { id: 'updates', level: 'Basic', minutes: 4, icon: RotateCcw, tags: 4, doNow: 4, avoid: 4, details: 3 },
        { id: 'browser-privacy', level: 'Smart', minutes: 8, icon: Globe, tags: 4, doNow: 4, avoid: 4, details: 3 },
        { id: 'vpn-tor', level: 'Smart', minutes: 6, icon: EyeOff, tags: 4, doNow: 4, avoid: 4, details: 3 },
        { id: 'phone-safety', level: 'Basic', minutes: 7, icon: Smartphone, tags: 4, doNow: 5, avoid: 4, details: 3 },
        { id: 'backups', level: 'Smart', minutes: 8, icon: HardDrive, tags: 4, doNow: 4, avoid: 4, details: 3 },
        { id: 'public-data', level: 'Smart', minutes: 10, icon: Search, tags: 4, doNow: 4, avoid: 4, details: 3 },
        { id: 'wifi-networks', level: 'Basic', minutes: 5, icon: Wifi, tags: 3, doNow: 4, avoid: 4, details: 3 },
        { id: 'email-recovery', level: 'Careful', minutes: 8, icon: Mail, tags: 3, doNow: 4, avoid: 4, details: 3 },
        { id: 'payments-shopping', level: 'Basic', minutes: 6, icon: CreditCard, tags: 3, doNow: 4, avoid: 4, details: 3 },
        { id: 'messaging', level: 'Smart', minutes: 7, icon: MessageSquareLock, tags: 3, doNow: 4, avoid: 4, details: 3 },
        { id: 'downloads-malware', level: 'Basic', minutes: 6, icon: Download, tags: 3, doNow: 4, avoid: 4, details: 3 },
        { id: 'device-disposal', level: 'Basic', minutes: 5, icon: Trash2, tags: 3, doNow: 4, avoid: 4, details: 3 },
        { id: 'screenshots-metadata', level: 'Smart', minutes: 6, icon: Camera, tags: 4, doNow: 4, avoid: 4, details: 3 },
        { id: 'ai-sharing', level: 'Smart', minutes: 6, icon: BrainCircuit, tags: 4, doNow: 4, avoid: 4, details: 3 },
        { id: 'security-planning', level: 'Basic', minutes: 6, icon: ShieldCheck, tags: 3, doNow: 4, avoid: 4, details: 3 },
        { id: 'data-brokers', level: 'Careful', minutes: 12, icon: Search, tags: 4, doNow: 4, avoid: 4, details: 3 },
        { id: 'email-aliases', level: 'Smart', minutes: 7, icon: Mail, tags: 4, doNow: 4, avoid: 4, details: 3 },
        { id: 'sim-swap', level: 'Careful', minutes: 8, icon: Smartphone, tags: 5, doNow: 4, avoid: 4, details: 3 },
        { id: 'home-router', level: 'Smart', minutes: 10, icon: Wifi, tags: 4, doNow: 4, avoid: 4, details: 3 },
        { id: 'cloud-storage', level: 'Smart', minutes: 8, icon: HardDrive, tags: 4, doNow: 4, avoid: 4, details: 3 },
        { id: 'social-media', level: 'Basic', minutes: 9, icon: Users, tags: 4, doNow: 4, avoid: 4, details: 3 },
        { id: 'location-privacy', level: 'Smart', minutes: 8, icon: Globe, tags: 4, doNow: 4, avoid: 4, details: 3 },
        { id: 'smart-home-iot', level: 'Smart', minutes: 9, icon: Settings2, tags: 4, doNow: 4, avoid: 4, details: 3 },
        { id: 'travel-security', level: 'Careful', minutes: 10, icon: Globe, tags: 4, doNow: 4, avoid: 4, details: 3 },
        { id: 'device-disposal-extra', level: 'Basic', minutes: 8, icon: Trash2, tags: 4, doNow: 4, avoid: 4, details: 3 },
        { id: 'incident-response', level: 'Careful', minutes: 10, icon: AlertTriangle, tags: 3, doNow: 4, avoid: 4, details: 3 },
        { id: 'qr-codes', level: 'Basic', minutes: 4, icon: FileText, tags: 4, doNow: 4, avoid: 4, details: 3 },
    ];

    /** Resolve a topic spec into a fully-translated `Topic` against the
     * active locale. The `$_` argument makes any `$derived` that calls
     * this re-run when the locale changes. */
    function resolveTopic(spec: TopicSpec, translate: typeof $_): Topic {
        const base = `page.privacyGuide.topics.${spec.id}`;
        const list = (field: string, n: number | undefined) =>
            n ? Array.from({ length: n }, (_v, i) => translate(`${base}.${field}.${i}`)) : undefined;
        return {
            id: spec.id,
            level: spec.level,
            minutes: spec.minutes,
            icon: spec.icon,
            title: translate(`${base}.title`),
            short: translate(`${base}.short`),
            takeaway: translate(`${base}.takeaway`),
            tags: list('tags', spec.tags) ?? [],
            doNow: list('doNow', spec.doNow) ?? [],
            avoid: list('avoid', spec.avoid) ?? [],
            details: list('details', spec.details) ?? [],
            scenarios: list('scenarios', spec.scenarios),
            examples: list('examples', spec.examples),
            oneClickFixes: list('oneClickFixes', spec.oneClickFixes),
            selfChecks: list('selfChecks', spec.selfChecks),
        };
    }

    /** Live, locale-resolved topic list. Re-derives whenever the
     * locale changes (the `$_` dependency inside `resolveTopic`). */
    let topics: Topic[] = $derived(topicSpecs.map((spec) => resolveTopic(spec, $_)));

    /** Twelve myth cards. No stable id — keyed by index in the bundle. */
    const MYTH_COUNT = 14;
    let myths: Myth[] = $derived(
        Array.from({ length: MYTH_COUNT }, (_v, i) => ({
            myth: $_(`page.privacyGuide.myths.${i}.myth`),
            reality: $_(`page.privacyGuide.myths.${i}.reality`),
        })),
    );

    const SOURCE_BASIS_COUNT = 4;
    let sourceBasis: string[] = $derived(
        Array.from({ length: SOURCE_BASIS_COUNT }, (_v, i) => $_(`page.privacyGuide.sourceBasis.${i}`)),
    );

    /** The microphone-privacy guarantees shown in the Local-data tab —
     *  #24 documents how KeepItLocal handles the mic right in the guide. */
    const MICROPHONE_POINT_COUNT = 5;
    let microphonePoints: string[] = $derived(
        Array.from({ length: MICROPHONE_POINT_COUNT }, (_v, i) =>
            $_(`page.privacyGuide.microphone.points.${i}`),
        ),
    );

    let activeTab = $state<Tab>('learn');
    let selectedTopicId = $state(topicSpecs[0]?.id ?? '');
    let articleTopicId = $state('');
    let searchQuery = $state('');
    let compact = $state(true);

    let normalizedQuery = $derived(searchQuery.trim().toLowerCase());

    let filteredTopics = $derived(
        normalizedQuery
            ? topics.filter((topic) => {
                const haystack = [
                    topic.title,
                    topic.short,
                    topic.level,
                    topic.tags.join(' '),
                    topic.takeaway,
                    topic.doNow.join(' '),
                    topic.avoid.join(' '),
                    topic.details.join(' ')
                ]
                    .join(' ')
                    .toLowerCase();
                return haystack.includes(normalizedQuery);
            })
            : topics
    );

    let selectedTopic = $derived(
        topics.find((topic) => topic.id === selectedTopicId) ?? filteredTopics[0] ?? topics[0]
    );
    let readingArticle = $derived(topics.find((topic) => topic.id === articleTopicId) ?? null);

    function articleScenarios(topic: Topic): string[] {
        if (topic.scenarios && topic.scenarios.length) return topic.scenarios;

        const tag = topic.tags[0] ?? $_('page.privacyGuide.article.fallbackArea');

        return [
            $_('page.privacyGuide.article.scenarioFallback.0', { values: { tag } }),
            $_('page.privacyGuide.article.scenarioFallback.1'),
            $_('page.privacyGuide.article.scenarioFallback.2'),
        ];
    }

    function articleExamples(topic: Topic): string[] {
        if (topic.examples && topic.examples.length) return topic.examples;

        const first = topic.doNow[0] ?? topic.short;
        const follow = topic.doNow.slice(1);
        const avoidExample = topic.avoid[0]
            ? $_('page.privacyGuide.article.exampleAvoid', { values: { text: topic.avoid[0] } })
            : undefined;

        return [
            $_('page.privacyGuide.article.exampleFirst', { values: { text: first } }),
            ...(avoidExample ? [avoidExample] : []),
            ...follow.slice(0, 2).map((item) => $_('page.privacyGuide.article.exampleInPractice', { values: { text: item } })),
        ];
    }

    function articleOneClickFixes(topic: Topic): string[] {
        if (topic.oneClickFixes && topic.oneClickFixes.length) return topic.oneClickFixes;

        return [
            ...topic.doNow,
            ...topic.avoid.slice(0, 1).map((item) => $_('page.privacyGuide.article.undoQuickly', { values: { text: item } })),
        ];
    }

    function articleChecks(topic: Topic): string[] {
        if (topic.selfChecks && topic.selfChecks.length) return topic.selfChecks;

        return [
            $_('page.privacyGuide.article.checkFallback.0'),
            $_('page.privacyGuide.article.checkFallback.1', { values: { tags: topic.tags.join(', ') } }),
            $_('page.privacyGuide.article.checkFallback.2'),
        ];
    }

    let topicArticleSections = $derived.by(() => {
        if (!readingArticle) return [] as ArticleSection[];

        return [
            {
                heading: $_('page.privacyGuide.article.sections.meaning'),
                points: [
                    $_('page.privacyGuide.article.meaningIntro', { values: { title: readingArticle.title } }),
                    readingArticle.takeaway,
                    $_('page.privacyGuide.article.levelMinutes', { values: { level: levelLabel(readingArticle.level), minutes: readingArticle.minutes } }),
                ],
            },
            {
                heading: $_('page.privacyGuide.article.sections.stuck'),
                points: readingArticle.details,
            },
            {
                heading: $_('page.privacyGuide.article.sections.scenarios'),
                points: articleScenarios(readingArticle),
            },
            {
                heading: $_('page.privacyGuide.article.sections.examples'),
                points: articleExamples(readingArticle),
            },
            {
                heading: $_('page.privacyGuide.article.sections.oneClick'),
                points: articleOneClickFixes(readingArticle).slice(0, 6),
            },
            {
                heading: $_('page.privacyGuide.article.sections.checks'),
                points: articleChecks(readingArticle),
            },
        ];
    });

    $effect(() => {
        if (filteredTopics.length > 0 && !filteredTopics.some((topic) => topic.id === selectedTopicId)) {
            selectedTopicId = filteredTopics[0].id;
        }
    });

    /** Ref to the topic-search input so the `/` keyboard shortcut can
     * focus it. Bound via the input's `bind:this` attribute. */
    let topicSearchInput: HTMLInputElement | null = $state(null);

    /** Global keyboard handler — `/` focuses the topic-search input
     * (matching the Vim / Discord / GitHub convention) unless the user
     * is already typing in an input. */
    function onPageKeydown(event: KeyboardEvent) {
        if (event.key !== '/') return;
        const target = event.target as HTMLElement | null;
        // Skip when the user is already typing somewhere — `/` should
        // produce a slash inside text fields, not jump focus.
        if (target && (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable)) {
            return;
        }
        if (topicSearchInput) {
            event.preventDefault();
            topicSearchInput.focus();
            topicSearchInput.select();
        }
    }

    onMount(() => {
        try {
            const densityRaw = localStorage.getItem(DENSITY_KEY);
            if (densityRaw) compact = densityRaw === 'true';
        } catch {
        }
        document.addEventListener('keydown', onPageKeydown);
        return () => {
            document.removeEventListener('keydown', onPageKeydown);
        };
    });

    function persistDensity(): void {
        try {
            localStorage.setItem(DENSITY_KEY, String(compact));
        } catch {
            // Ignore storage failures.
        }
    }

    function openTopic(id: string): void {
        selectedTopicId = id;
        articleTopicId = '';
        activeTab = 'learn';
    }

    function openTopicArticle(id: string): void {
        selectedTopicId = id;
        articleTopicId = id;
        activeTab = 'learn';
    }

    function goHomeFromArticle(): void {
        articleTopicId = '';
        selectedTopicId = topics[0]?.id ?? '';
    }

    function toggleDensity(): void {
        compact = !compact;
        persistDensity();
    }

    function levelClass(level: Level): string {
        if (level === 'Basic') return 'border-success-strong bg-success-soft text-success';
        if (level === 'Smart') return 'border-info-strong bg-info-soft text-info';
        return 'border-warning-strong bg-warning-soft text-warning';
    }

    /** Localized display label for a difficulty level. The `Level`
     * literal stays English (used by `levelClass`); this maps it to the
     * user-facing string. */
    function levelLabel(level: Level): string {
        return $_(`page.privacyGuide.levels.${level.toLowerCase()}`);
    }

</script>

<ToolPage
    icon={ShieldCheck}
    iconTint="var(--color-accent)"
    title={$_('page.privacyGuide.heroTitle')}
    description={$_('page.privacyGuide.heroSubtitle')}
    width="wide"
    fill={false}
>
    <div class="pg-content {compact ? 'space-y-3' : 'space-y-5'}" class:is-compact={compact}>
        <div class="pg-overview grid gap-3 sm:grid-cols-2">
                    <div class="pg-stat rounded-2xl border border-border bg-panel-2 p-4">
                        <div class="text-xs uppercase tracking-wide text-muted">{$_('page.privacyGuide.stats.guideTopics')}</div>
                        <div class="mt-2 text-2xl font-semibold text-text">{topics.length}</div>
                        <p class="mt-2 text-xs leading-5 text-muted">{$_('page.privacyGuide.stats.guideTopicsHint')}</p>
                    </div>

                    <div class="pg-stat rounded-2xl border border-border bg-panel-2 p-4">
                        <div class="text-xs uppercase tracking-wide text-muted">{$_('page.privacyGuide.stats.localMode')}</div>
                        <div class="mt-2 text-sm font-semibold text-text">{$_('page.privacyGuide.stats.localModeTitle')}</div>
                        <p class="mt-2 text-xs leading-5 text-muted">{$_('page.privacyGuide.stats.localModeHint')}</p>
                    </div>
        </div>

        <div class="pg-toolbar relative mt-5 flex flex-col gap-3 rounded-2xl border border-border bg-panel-2 p-2 lg:flex-row lg:items-center lg:justify-between">
                <div class="pg-tabs flex flex-wrap gap-1">
                    {#each [
                        { id: 'learn', label: 'page.privacyGuide.tabs.learn', icon: BookOpen },
                        { id: 'myths', label: 'page.privacyGuide.tabs.myths', icon: Lightbulb },
                        { id: 'privacy', label: 'page.privacyGuide.tabs.privacy', icon: Info }
                    ] as tab}
                        <button
                            type="button"
                            class="pg-tab flex items-center gap-1.5 rounded-xl px-3 py-2 text-sm transition-colors {activeTab === tab.id ? 'is-active' : ''}"
                            onclick={() => (activeTab = tab.id as Tab)}
                        >
                            <tab.icon class="h-3.5 w-3.5" />
                            {$_(tab.label)}
                        </button>
                    {/each}
                </div>

                <div class="pg-toolbar-actions flex flex-col gap-2 sm:flex-row sm:items-center">
                    {#if activeTab === 'learn'}
                        <div class="pg-search relative min-w-0 sm:w-72">
                            <Search class="pointer-events-none absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted" />
                            <input
                                bind:this={topicSearchInput}
                                bind:value={searchQuery}
                                use:escToClear={() => (searchQuery = '')}
                                class="pg-search-input h-10 w-full rounded-xl border border-border bg-panel-2 pl-9 pr-3 text-sm outline-none transition-colors focus:border-accent"
                                placeholder={$_('page.privacyGuide.searchTopicsPlaceholder')}
                            />
                        </div>
                    {/if}
                    <button
                        type="button"
                        class="pg-density h-10 rounded-xl border border-border bg-panel-2 px-3 text-sm text-muted hover:border-accent/70 hover:text-text"
                        onclick={toggleDensity}
                    >
                        {compact ? $_('page.privacyGuide.comfortView') : $_('page.privacyGuide.compactView')}
                    </button>
                </div>
            </div>

    {#if activeTab === 'learn'}
        <div class="pg-learn-workspace grid gap-3 lg:grid-cols-[300px_1fr]">
            <aside class="pg-topic-sidebar rounded border border-border bg-panel p-2">
                <div class="mb-2 flex items-center justify-between px-1 text-xs uppercase tracking-wider text-muted">
                    <span>{$_('page.privacyGuide.topicsLabel')}</span>
                    <span>{filteredTopics.length}/{topics.length}</span>
                </div>

                <div class="max-h-[calc(100vh-240px)] min-h-[360px] space-y-1 overflow-auto pr-1">
                    {#if filteredTopics.length === 0}
                        <div class="pg-empty rounded border border-border border-dashed bg-panel-2/40 p-4 text-center text-xs text-muted">
                            {$_('page.privacyGuide.noTopicsMatch')} <strong class="text-text">"{searchQuery}"</strong>.
                            <button
                                type="button"
                                onclick={() => (searchQuery = '')}
                                class="mt-2 block w-full text-accent underline-offset-2 hover:underline"
                            >
                                {$_('page.privacyGuide.clearSearch')}
                            </button>
                        </div>
                    {/if}
                    {#each filteredTopics as topic}
                        <button
                                type="button"
                                class="pg-topic-row w-full rounded border p-2 text-left transition-colors {selectedTopic?.id === topic.id ? 'is-selected' : ''}"
                                onclick={() => openTopic(topic.id)}
                        >
                            <div class="flex items-start gap-2">
                                <div class="pg-topic-icon mt-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded bg-bg text-muted">
                                    <topic.icon class="h-3.5 w-3.5" />
                                </div>
                                <div class="min-w-0 flex-1">
                                    <div class="flex items-center justify-between gap-2">
                                        <span class="truncate text-sm font-medium text-text">{topic.title}</span>
                                        <span class="shrink-0 text-[11px] text-muted">{$_('page.privacyGuide.minutesShort', { values: { minutes: topic.minutes } })}</span>
                                    </div>
                                    <p class="mt-0.5 line-clamp-2 text-xs leading-4 text-muted">{topic.short}</p>
                                </div>
                            </div>
                        </button>
                    {/each}
                </div>
            </aside>

            {#if readingArticle}
                <article class="pg-reading-panel rounded border border-border bg-panel p-4">
                    <button
                        type="button"
                        class="pg-back mb-3 inline-flex items-center gap-2 rounded border border-border bg-panel-2 px-3 py-1.5 text-xs hover:border-accent hover:text-accent"
                        onclick={goHomeFromArticle}
                    >
                        <ArrowLeft class="h-3.5 w-3.5" />
                        {$_('page.privacyGuide.backToHome')}
                    </button>

                    <div class="pg-article-heading mb-3 flex flex-col gap-3 md:flex-row md:items-start md:justify-between">
                        <div class="flex min-w-0 gap-3">
                            <div class="flex h-10 w-10 shrink-0 items-center justify-center rounded bg-transparent text-accent">
                                <readingArticle.icon class="h-5 w-5" />
                            </div>
                            <div class="min-w-0">
                                <h2 class="text-xl font-semibold tracking-tight">{readingArticle.title}</h2>
                                <p class="mt-1 text-sm text-muted">{readingArticle.short}</p>
                            </div>
                        </div>
                        <div class="flex shrink-0 flex-wrap gap-2">
                            <span class="rounded border px-2 py-1 text-xs {levelClass(readingArticle.level)}">{levelLabel(readingArticle.level)}</span>
                            <span class="rounded border border-border bg-panel-2 px-2 py-1 text-xs text-muted">{$_('page.privacyGuide.minutesLong', { values: { minutes: readingArticle.minutes } })}</span>
                        </div>
                    </div>

                    <div class="pg-main-idea rounded border border-success-strong bg-success-soft p-3 text-sm leading-6 text-text-secondary">
                        <strong class="text-text">{$_('page.privacyGuide.mainIdea')}</strong> {readingArticle.takeaway}
                    </div>

                    <div class="mt-4 grid gap-3">
                        {#each topicArticleSections as section}
                            <section class="pg-article-section rounded border border-border bg-bg/30 p-3">
                                <h3 class="mb-2 text-sm font-semibold">{section.heading}</h3>
                                <ul class="space-y-1.5 text-sm leading-5 text-muted">
                                    {#each section.points as point}
                                        <li class="flex gap-2"><span class="text-success">-</span><span>{point}</span></li>
                                    {/each}
                                </ul>
                            </section>
                        {/each}
                    </div>

                    <div class="mt-4 flex flex-wrap gap-1.5">
                        {#each readingArticle.tags as tag}
                            <span class="rounded border border-border bg-panel-2 px-2 py-1 text-[11px] text-muted">#{tag}</span>
                        {/each}
                    </div>
                </article>
            {:else if selectedTopic}
                <article class="pg-reading-panel rounded border border-border bg-panel p-4">
                    <div class="pg-article-heading mb-3 flex flex-col gap-3 md:flex-row md:items-start md:justify-between">
                        <div class="flex min-w-0 gap-3">
                            <div class="flex h-10 w-10 shrink-0 items-center justify-center rounded bg-transparent text-accent">
                                <selectedTopic.icon class="h-5 w-5" />
                            </div>
                            <div class="min-w-0">
                                <h2 class="text-xl font-semibold tracking-tight">{selectedTopic.title}</h2>
                                <p class="mt-1 text-sm text-muted">{selectedTopic.short}</p>
                            </div>
                        </div>
                        <div class="flex shrink-0 flex-wrap gap-2">
                            <span class="rounded border px-2 py-1 text-xs {levelClass(selectedTopic.level)}">{levelLabel(selectedTopic.level)}</span>
                            <span class="rounded border border-border bg-panel-2 px-2 py-1 text-xs text-muted">{$_('page.privacyGuide.minutesLong', { values: { minutes: selectedTopic.minutes } })}</span>
                        </div>
                    </div>

                    <div class="pg-main-idea rounded border border-border bg-bg/50 p-3 text-sm leading-6 text-text-secondary">
                        <strong class="text-text">{$_('page.privacyGuide.mainIdea')}</strong> {selectedTopic.takeaway}
                    </div>

                    <div class="mt-3 grid gap-3 xl:grid-cols-2">
                        <section class="pg-guidance is-do rounded border border-success-strong bg-success-soft p-3">
                            <h3 class="mb-2 flex items-center gap-2 text-sm font-semibold text-success">
                                <CheckCircle2 class="h-4 w-4" />
                                {$_('page.privacyGuide.doNow')}
                            </h3>
                            <ul class="space-y-1.5 text-sm leading-5 text-text-secondary">
                                {#each selectedTopic.doNow as item}
                                    <li class="flex gap-2"><span class="text-success">✓</span><span>{item}</span></li>
                                {/each}
                            </ul>
                        </section>

                        <section class="pg-guidance is-avoid rounded border border-error-strong bg-error-soft p-3">
                            <h3 class="mb-2 flex items-center gap-2 text-sm font-semibold text-error">
                                <AlertTriangle class="h-4 w-4" />
                                {$_('page.privacyGuide.avoid')}
                            </h3>
                            <ul class="space-y-1.5 text-sm leading-5 text-text-secondary">
                                {#each selectedTopic.avoid as item}
                                    <li class="flex gap-2"><span class="text-error">×</span><span>{item}</span></li>
                                {/each}
                            </ul>
                        </section>
                    </div>

                    <section class="pg-details mt-3 rounded border border-border bg-bg/30 p-3">
                        <h3 class="mb-2 text-sm font-semibold">{$_('page.privacyGuide.whyItMatters')}</h3>
                        <ul class="grid gap-2 text-sm leading-5 text-muted {compact ? 'xl:grid-cols-3' : 'xl:grid-cols-2'}">
                            {#each selectedTopic.details as item}
                                <li class="rounded border border-border bg-panel p-2">{item}</li>
                            {/each}
                        </ul>
                    </section>

                    <div class="mt-3 flex flex-wrap gap-1.5">
                        <button
                            type="button"
                            class="pg-read-more inline-flex items-center gap-1 rounded border border-accent px-2.5 py-1 text-xs font-medium text-accent"
                            onclick={() => openTopicArticle(selectedTopic.id)}
                        >
                            {$_('page.privacyGuide.readMore')}
                        </button>
                        {#each selectedTopic.tags as tag}
                            <span class="rounded border border-border bg-panel-2 px-2 py-1 text-[11px] text-muted">#{tag}</span>
                        {/each}
                    </div>
                </article>
            {/if}
        </div>
    {:else if activeTab === 'myths'}
        <section class="pg-myth-grid grid gap-3 md:grid-cols-2 xl:grid-cols-4">
            {#each myths as item}
                <article class="pg-myth-card rounded border border-border bg-panel p-3">
                    <h3 class="text-sm font-semibold">{item.myth}</h3>
                    <p class="mt-2 text-sm leading-5 text-muted">{item.reality}</p>
                </article>
            {/each}
        </section>
    {:else}
        <div class="pg-local-workspace grid gap-3 lg:grid-cols-[1fr_360px]">
            <div class="space-y-3">
            <section class="pg-local-panel rounded border border-border bg-panel p-4">
                <h2 class="mb-2 flex items-center gap-2 text-base font-semibold">
                    <FileText class="h-4 w-4 text-accent" />
                    {$_('page.privacyGuide.localHub.title')}
                </h2>
                <p class="max-w-3xl text-sm leading-6 text-muted">
                    {$_('page.privacyGuide.localHub.desc')}
                </p>

                <div class="mt-4 grid gap-3 md:grid-cols-3">
                    <div class="pg-local-card rounded border border-border bg-bg/30 p-3">
                        <div class="mb-1 flex items-center gap-2 text-sm font-medium"><Lock class="h-4 w-4 text-accent" />{$_('page.privacyGuide.localHub.noNetwork.title')}</div>
                        <p class="text-xs leading-5 text-muted">{$_('page.privacyGuide.localHub.noNetwork.text')}</p>
                    </div>
                    <div class="pg-local-card rounded border border-border bg-bg/30 p-3">
                        <div class="mb-1 flex items-center gap-2 text-sm font-medium"><Settings2 class="h-4 w-4 text-accent" />{$_('page.privacyGuide.localHub.localProgress.title')}</div>
                        <p class="text-xs leading-5 text-muted">{$_('page.privacyGuide.localHub.localProgress.text')}</p>
                    </div>
                    <div class="pg-local-card rounded border border-border bg-bg/30 p-3">
                        <div class="mb-1 flex items-center gap-2 text-sm font-medium"><Users class="h-4 w-4 text-accent" />{$_('page.privacyGuide.localHub.normalPeople.title')}</div>
                        <p class="text-xs leading-5 text-muted">{$_('page.privacyGuide.localHub.normalPeople.text')}</p>
                    </div>
                </div>
            </section>

            <section class="pg-local-panel rounded border border-border bg-panel p-4">
                <h2 class="mb-2 flex items-center gap-2 text-base font-semibold">
                    <Mic class="h-4 w-4 text-accent" />
                    {$_('page.privacyGuide.microphone.title')}
                </h2>
                <p class="max-w-3xl text-sm leading-6 text-muted">
                    {$_('page.privacyGuide.microphone.intro')}
                </p>
                <ul class="mt-3 grid gap-2">
                    {#each microphonePoints as point}
                        <li class="pg-mic-point flex gap-2 rounded border border-border bg-bg/30 p-2 text-sm leading-5 text-muted">
                            <ShieldCheck class="mt-0.5 h-4 w-4 shrink-0 text-success" />
                            <span>{point}</span>
                        </li>
                    {/each}
                </ul>
            </section>
            </div>

            <aside class="pg-source-panel rounded border border-border bg-panel p-4">
                <h3 class="mb-2 text-sm font-semibold">{$_('page.privacyGuide.sourceBasisTitle')}</h3>
                <ul class="space-y-2 text-xs leading-5 text-muted">
                    {#each sourceBasis as source}
                        <li class="pg-source-item rounded border border-border bg-bg/30 p-2">{source}</li>
                    {/each}
                </ul>
                <div class="pg-local-note mt-3 rounded border border-border bg-panel-2 p-2 text-xs leading-5 text-muted">
                    <strong class="text-text">{$_('page.privacyGuide.localHub.footer')}</strong>
                </div>
            </aside>
        </div>
    {/if}
    </div>
</ToolPage>

<style>
    /* Calm workspace treatment for the guide's existing learning flows. */
    .pg-content {
        --pg-gap: 14px;
        display: grid;
        gap: var(--pg-gap);
    }
    .pg-content > * {
        margin-top: 0 !important;
        margin-bottom: 0 !important;
    }
    .pg-content.is-compact {
        --pg-gap: 10px;
    }
    .pg-content.is-compact .pg-stat,
    .pg-content.is-compact .pg-reading-panel,
    .pg-content.is-compact .pg-local-panel,
    .pg-content.is-compact .pg-source-panel {
        padding: 10px !important;
    }

    .pg-stat,
    .pg-toolbar,
    .pg-topic-sidebar,
    .pg-reading-panel,
    .pg-myth-card,
    .pg-local-panel,
    .pg-source-panel {
        border-color: var(--color-border) !important;
        border-radius: var(--radius-card, 12px);
        background: var(--color-panel) !important;
        box-shadow: inset 0 1px 0 color-mix(in srgb, var(--color-text) 6%, transparent);
    }

    .pg-stat {
        position: relative;
        min-height: 108px;
        overflow: hidden;
    }
    .pg-stat::before {
        position: absolute;
        top: 0;
        left: 14px;
        width: 28px;
        height: 2px;
        border-radius: 0 0 999px 999px;
        background: var(--color-accent);
        content: '';
    }
    .pg-stat > :first-child,
    .pg-topic-sidebar > :first-child {
        font-size: 10.5px !important;
        font-weight: 700;
        letter-spacing: .08em;
    }
    .pg-stat :global(.bg-panel-2) {
        background: color-mix(in srgb, var(--color-text) 6%, transparent);
    }

    .pg-toolbar {
        display: flex;
        align-items: center;
        padding: 7px !important;
        border-radius: var(--radius-card, 12px) !important;
    }
    .pg-tabs {
        align-items: center;
    }
    .pg-tab {
        position: relative;
        min-height: 36px;
        border: 1px solid transparent;
        background: transparent;
        color: var(--color-muted);
        font: inherit;
        font-size: 12px !important;
        line-height: 1;
        cursor: pointer;
    }
    .pg-tab:hover {
        background: color-mix(in srgb, var(--color-text) 4%, transparent);
        color: var(--color-text);
    }
    .pg-tab.is-active {
        padding-left: 15px;
        border-color: color-mix(in srgb, var(--color-accent) 40%, var(--color-border));
        background: var(--color-panel-2);
        color: var(--color-text);
        font-weight: 600;
    }
    .pg-tab.is-active::before,
    .pg-topic-row.is-selected::before {
        position: absolute;
        left: 6px;
        top: 8px;
        bottom: 8px;
        width: 3px;
        border-radius: 999px;
        background: var(--color-accent);
        content: '';
    }
    .pg-toolbar-actions {
        align-items: center;
    }
    .pg-search-input {
        border-color: var(--color-border) !important;
        border-radius: var(--radius-control, 8px) !important;
        background: var(--color-panel-2) !important;
        color: var(--color-text);
        box-shadow: inset 0 1px 0 color-mix(in srgb, var(--color-text) 5%, transparent);
    }
    .pg-search-input:focus {
        border-color: var(--color-accent) !important;
        box-shadow: 0 0 0 3px color-mix(in srgb, var(--color-accent) 13%, transparent);
    }
    .pg-density,
    .pg-back,
    .pg-read-more {
        border-color: var(--color-border) !important;
        border-radius: var(--radius-control, 8px) !important;
        background: var(--color-panel-2) !important;
        color: var(--color-text-secondary);
        font-weight: 600;
    }
    .pg-density:hover,
    .pg-back:hover,
    .pg-read-more:hover {
        border-color: color-mix(in srgb, var(--color-accent) 48%, var(--color-border)) !important;
        color: var(--color-text) !important;
    }
    .pg-read-more {
        border-color: color-mix(in srgb, var(--color-accent) 45%, var(--color-border)) !important;
        background: transparent !important;
        color: var(--color-accent) !important;
    }
    .pg-learn-workspace,
    .pg-local-workspace {
        align-items: start;
    }
    .pg-topic-sidebar {
        padding: 9px !important;
    }
    .pg-topic-sidebar :global(.max-h-\[calc\(100vh-240px\)\]) {
        scrollbar-color: color-mix(in srgb, var(--color-text) 18%, transparent) transparent;
    }
    .pg-empty {
        border-color: color-mix(in srgb, var(--color-text) 14%, var(--color-border)) !important;
        border-radius: var(--radius-control, 8px) !important;
        background: var(--color-panel-2) !important;
    }
    .pg-topic-row {
        position: relative;
        min-height: 62px;
        border-color: transparent !important;
        background: transparent;
        color: var(--color-text);
    }
    .pg-topic-row:hover {
        border-color: var(--color-border) !important;
        background: color-mix(in srgb, var(--color-text) 4%, transparent);
    }
    .pg-topic-row.is-selected {
        padding-left: 14px !important;
        border-color: color-mix(in srgb, var(--color-accent) 46%, var(--color-border)) !important;
        background: var(--color-panel-2);
    }
    .pg-topic-icon {
        background: transparent !important;
        color: var(--color-muted);
        box-shadow:
            inset 0 1px 0 color-mix(in srgb, var(--color-text) 10%, transparent),
            inset 0 0 0 1px color-mix(in srgb, var(--color-text) 5%, transparent);
    }
    .pg-topic-row.is-selected .pg-topic-icon {
        color: var(--color-accent);
    }
    .pg-reading-panel {
        padding: 16px !important;
    }
    .pg-article-heading > div:first-child > div:first-child {
        background: transparent !important;
        box-shadow:
            inset 0 1px 0 color-mix(in srgb, var(--color-text) 10%, transparent),
            inset 0 0 0 1px color-mix(in srgb, var(--color-text) 5%, transparent);
    }
    .pg-main-idea,
    .pg-article-section,
    .pg-details {
        position: relative;
        border-color: var(--color-border) !important;
        border-radius: var(--radius-control, 8px) !important;
        background: var(--color-panel-2) !important;
    }
    .pg-main-idea {
        padding-left: 15px !important;
    }
    .pg-main-idea::before {
        position: absolute;
        top: 10px;
        bottom: 10px;
        left: 6px;
        width: 3px;
        border-radius: 999px;
        background: var(--color-accent);
        content: '';
    }
    .pg-article-section h3,
    .pg-details h3 {
        color: var(--color-text);
        font-size: 12px !important;
        font-weight: 700;
        letter-spacing: .01em;
    }
    .pg-guidance {
        position: relative;
        overflow: hidden;
        border-color: var(--color-border) !important;
        border-radius: var(--radius-control, 8px) !important;
        background: var(--color-panel-2) !important;
    }
    .pg-guidance::before {
        position: absolute;
        top: 0;
        bottom: 0;
        left: 0;
        width: 3px;
        content: '';
    }
    .pg-guidance.is-do::before {
        background: var(--color-success);
    }
    .pg-guidance.is-avoid::before {
        background: var(--color-error);
    }
    .pg-details li {
        border-radius: var(--radius-control, 8px);
        background: var(--color-panel) !important;
    }

    .pg-local-panel,
    .pg-source-panel {
        padding: 14px !important;
    }

    .pg-myth-grid {
        gap: 10px !important;
    }
    .pg-myth-card {
        min-height: 132px;
        padding: 14px !important;
        border-left: 3px solid color-mix(in srgb, var(--color-accent) 65%, var(--color-border));
    }
    .pg-myth-card h3 {
        color: var(--color-text-secondary);
    }
    .pg-myth-card p {
        color: var(--color-text) !important;
    }

    .pg-local-card,
    .pg-mic-point,
    .pg-source-item,
    .pg-local-note {
        border-color: var(--color-border) !important;
        border-radius: var(--radius-control, 8px) !important;
        background: var(--color-panel-2) !important;
    }
    .pg-local-card {
        min-height: 112px;
    }
    .pg-mic-point {
        border-left: 3px solid color-mix(in srgb, var(--color-success) 62%, var(--color-border)) !important;
    }
    .pg-source-item {
        padding: 10px !important;
    }
    .pg-local-note {
        border-left: 3px solid var(--color-accent) !important;
    }

    .pg-tab:focus-visible,
    .pg-density:focus-visible,
    .pg-topic-row:focus-visible,
    .pg-back:focus-visible,
    .pg-read-more:focus-visible {
        outline: 2px solid var(--color-accent);
        outline-offset: 2px;
    }

    @media (max-width: 720px) {
        .pg-toolbar-actions {
            width: 100%;
        }
        .pg-search {
            flex: 1;
        }
        .pg-density {
            flex: none;
        }
        .pg-stat {
            min-height: 92px;
        }
        .pg-reading-panel,
        .pg-local-panel,
        .pg-source-panel {
            padding: 12px !important;
        }
    }

    @media (prefers-reduced-motion: reduce) {
        .pg-tab,
        .pg-topic-row {
            transition: none !important;
        }
    }
</style>

