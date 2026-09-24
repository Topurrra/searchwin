<script lang="ts">
    import { onDestroy, onMount } from 'svelte';
    import { convertFileSrc, invoke } from '@tauri-apps/api/core';
    import { get } from 'svelte/store';
    import {
        Activity,
        AlertTriangle,
        AppWindow,
        BarChart3,
        Briefcase,
        CalendarDays,
        ChevronLeft,
        ChevronRight,
        Circle,
        Clock3,
        Code2,
        EyeOff,
        Film,
        Gamepad2,
        Globe,
        LayoutDashboard,
        MessageCircle,
        Monitor,
        Palette,
        Pause,
        Play,
        Plus,
        Settings,
        ShieldCheck,
        Target,
        Trash2,
        X,
    } from '@lucide/svelte';
    import { Button, Select, Tabs, TextInput, Toggle, ToolPage, ToolPanel, ToolToolbar } from '$lib/ui';
    import { humanizeAppName, sourceColor } from '$lib/utils/appNames';
    import type { LaunchTargetItem, LaunchTargetSearchResult } from '$lib/stores/fileSearch';
    import {
        getConfig,
        setConfig,
        getStatus,
        getRange,
        pauseTracking,
        wipeData,
        todayKey,
        addDays,
        weekDates,
        weekdayLabel,
        prettyDate,
        parseDate,
        formatDuration,
        timeTrackerTab,
        timeTrackerView,
        timeTrackerRefDate,
        type TimeTrackerTab,
        type TimeTrackerView,
        type TrackerConfig,
        type TrackerStatus,
        type RangeSummary,
        type CategoryDef,
        type CategoryRule,
        type Goal,
    } from '$lib/stores/timeTracker';

    type TabId = TimeTrackerTab;
    type ViewId = TimeTrackerView;

    const trackerTabs = [
        { id: 'dashboard', label: 'Overview', icon: LayoutDashboard },
        { id: 'settings', label: 'Settings', icon: Settings },
    ];
    const rangeTabs = [
        { id: 'day', label: 'Day', icon: CalendarDays },
        { id: 'week', label: 'Week', icon: BarChart3 },
    ];
    const categoryKindOptions = [
        { value: 'productive', label: 'Productive' },
        { value: 'neutral', label: 'Neutral' },
        { value: 'distracting', label: 'Distracting' },
    ];
    const ruleFieldOptions = [
        { value: 'app', label: 'App' },
        { value: 'title', label: 'Window title' },
    ];
    const ruleOperatorOptions = [
        { value: 'contains', label: 'Contains' },
        { value: 'is', label: 'Is exactly' },
    ];

    let config = $state<TrackerConfig | null>(null);
    let status = $state<TrackerStatus | null>(null);
    let summary = $state<RangeSummary | null>(null);
    let loading = $state(false);
    let view = $state<ViewId>(get(timeTrackerView));
    let refDate = $state(get(timeTrackerRefDate));
    let tab = $state<TabId>(get(timeTrackerTab));
    let wipeArmed = $state(false);
    let savedTick = $state(false);
    let newExclude = $state('');
    let appIcons = $state<Record<string, string | null>>({});

    const dates = $derived(view === 'day' ? [refDate] : weekDates(refDate));
    const isToday = $derived(view === 'day' && refDate === todayKey());
    const isCurrentRange = $derived(dates.includes(todayKey()));
    const categoryOptions = $derived(
        config?.categories.map((category) => ({ value: category.name, label: category.name })) ?? [],
    );

    async function load(forDates: string[]) {
        loading = true;
        try {
            summary = await getRange(forDates);
        } catch (error) {
            console.error('time tracker range failed', error);
        } finally {
            loading = false;
        }
    }

    async function refreshStatus() {
        try {
            status = await getStatus();
        } catch {
            /* Tracker startup can briefly race the first poll. */
        }
    }

    $effect(() => {
        void load(dates);
    });

    let statusTimer: ReturnType<typeof setInterval> | null = null;
    let liveTimer: ReturnType<typeof setInterval> | null = null;
    onMount(async () => {
        config = await getConfig().catch(() => null);
        await refreshStatus();
        statusTimer = setInterval(refreshStatus, 5000);
        liveTimer = setInterval(() => {
            if (isToday || view === 'week') void load(dates);
        }, 20000);
    });
    onDestroy(() => {
        if (statusTimer) clearInterval(statusTimer);
        if (liveTimer) clearInterval(liveTimer);
    });

    const rangeLabel = $derived.by(() => {
        if (view === 'day') return prettyDate(refDate);
        const week = weekDates(refDate);
        const start = parseDate(week[0]);
        const end = parseDate(week[6]);
        const options: Intl.DateTimeFormatOptions = { month: 'short', day: 'numeric' };
        return start.toLocaleDateString(undefined, options) + ' to ' + end.toLocaleDateString(undefined, options);
    });

    const statusTitle = $derived.by(() => {
        if (!status) return 'Checking tracker status';
        if (status.tracking) {
            return status.currentApp
                ? 'Recording ' + humanizeAppName(status.currentApp)
                : 'Recording activity';
        }
        return status.paused ? 'Tracking is paused' : 'Tracking is off';
    });

    const statusDescription = $derived.by(() => {
        if (!status) return 'Your local tracker is being checked.';
        if (status.tracking) {
            return status.currentCategory
                ? 'Classified as ' + status.currentCategory + '. Everything stays on this device.'
                : 'Your activity stays on this device and is grouped automatically.';
        }
        if (status.paused) return 'Resume when you are ready to record again.';
        return 'Turn on tracking when you want your activity journal to begin.';
    });

    const colorFor = (name: string) =>
        summary?.categories.find((category) => category.name === name)?.color ?? '#94a3b8';

    const dayStartMs = $derived(parseDate(refDate).getTime());
    const DAY_MS = 86_400_000;

    interface Seg {
        leftPct: number;
        widthPct: number;
        color: string;
        idle: boolean;
        label: string;
        app: string;
        title: string;
        category: string;
        startMs: number;
        endMs: number;
    }

    let selectedSegment = $state<Seg | null>(null);

    const timelineSegs = $derived.by<Seg[]>(() => {
        if (!summary || view !== 'day') return [];
        const segments: Seg[] = [];
        for (const event of summary.timeline) {
            const start = Math.max(event.startMs, dayStartMs);
            const end = Math.min(event.endMs, dayStartMs + DAY_MS);
            if (end <= start) continue;

            const widthPct = ((end - start) / DAY_MS) * 100;
            if (widthPct < 0.05) continue;

            segments.push({
                leftPct: ((start - dayStartMs) / DAY_MS) * 100,
                widthPct,
                color: event.idle ? '#475569' : colorFor(event.category),
                idle: event.idle,
                label:
                    (event.idle ? 'Away' : event.app || event.category) +
                    (event.title ? ': ' + event.title : ''),
                app: event.app,
                title: event.title,
                category: event.category,
                startMs: start,
                endMs: end,
            });
        }
        return segments;
    });

    const timelineDescription = $derived.by(() => {
        if (view !== 'day') return '';
        if (!timelineSegs.length) return 'No recorded activity in this day.';
        return timelineSegs.length + ' recorded activity blocks. Activity colors match their category.';
    });

    const nowPct = $derived(isToday ? ((Date.now() - dayStartMs) / DAY_MS) * 100 : -1);
    const weekMax = $derived(
        Math.max(1, ...(summary?.byDay.map((day) => day.activeSecs + day.idleSecs) ?? [1])),
    );
    const catMax = $derived(Math.max(1, ...(summary?.byCategory.map((category) => category.secs) ?? [1])));
    const appMax = $derived(Math.max(1, ...(summary?.byApp.map((app) => app.secs) ?? [1])));
    const topApps = $derived(summary?.byApp.slice(0, 8) ?? []);
    const topCategory = $derived(summary?.byCategory[0] ?? null);
    const focusColor = $derived.by(() => {
        const score = summary?.focusScore ?? 0;
        if (score >= 60) return '#10b981';
        if (score >= 35) return '#f59e0b';
        return '#ef4444';
    });
    const idleMinutes = $derived(config ? Math.round(config.idleThresholdSecs / 60) : 3);

    function percent(value: number, max: number) {
        if (max <= 0) return 0;
        return Math.min(100, Math.max(0, (value / max) * 100));
    }

    function appKey(value: string) {
        return value
            .toLowerCase()
            .replace(/\.exe$/i, '')
            .replace(/[^a-z0-9]/g, '');
    }

    function bestLaunchTarget(app: string, results: LaunchTargetItem[]) {
        const key = appKey(app);
        return (
            results.find((target) => {
                const fileName = target.path.split(/[\\/]/).pop() ?? '';
                return (
                    appKey(fileName.replace(/\.(exe|lnk)$/i, '')) === key ||
                    appKey(target.name) === key
                );
            }) ?? results[0]
        );
    }

    async function fetchAppIcon(app: string) {
        const key = appKey(app);
        if (!key || key in appIcons) return;

        appIcons = { ...appIcons, [key]: null };
        try {
            const result = await invoke<LaunchTargetSearchResult>('search_launch_targets', {
                options: { query: humanizeAppName(app), limit: 5 },
            });
            const target = bestLaunchTarget(app, result.results);
            if (!target?.path) return;

            const iconPath = await invoke<string | null>('ensure_launcher_icon', {
                path: target.path,
                kind: 'app',
            });
            if (iconPath) {
                appIcons = { ...appIcons, [key]: convertFileSrc(iconPath) };
            }
        } catch {
            // Keep the application glyph when a launcher icon is unavailable.
        }
    }

    $effect(() => {
        if (tab !== 'dashboard') return;
        for (const app of topApps) {
            void fetchAppIcon(app.app);
        }
    });

    function categoryIconFor(category: CategoryDef) {
        switch (category.name.trim().toLowerCase()) {
            case 'development': return Code2;
            case 'office': return Briefcase;
            case 'design': return Palette;
            case 'communication': return MessageCircle;
            case 'browsing': return Globe;
            case 'system': return Monitor;
            case 'media': return Film;
            case 'games': return Gamepad2;
        }
        if (category.kind === 'productive') return Target;
        if (category.kind === 'distracting') return AlertTriangle;
        return Circle;
    }

    function selectSegment(segment: Seg) {
        const isSelected =
            selectedSegment?.startMs === segment.startMs &&
            selectedSegment.endMs === segment.endMs &&
            selectedSegment.app === segment.app;
        selectedSegment = isSelected ? null : segment;
    }

    function clockTime(timestamp: number) {
        return new Date(timestamp).toLocaleTimeString(undefined, {
            hour: 'numeric',
            minute: '2-digit',
        });
    }

    function openDay(date: string) {
        selectedSegment = null;
        view = 'day';
        timeTrackerView.set(view);
        refDate = date;
        timeTrackerRefDate.set(refDate);
    }

    function shiftDate(delta: number) {
        selectedSegment = null;
        refDate = view === 'day' ? addDays(refDate, delta) : addDays(refDate, delta * 7);
        timeTrackerRefDate.set(refDate);
    }

    function goToday() {
        selectedSegment = null;
        refDate = todayKey();
        timeTrackerRefDate.set(refDate);
    }

    async function persist() {
        if (!config) return;
        try {
            await setConfig($state.snapshot(config));
            savedTick = true;
            setTimeout(() => (savedTick = false), 1400);
            await refreshStatus();
        } catch (error) {
            console.error('save time tracker config failed', error);
        }
    }

    async function setEnabled(enabled: boolean) {
        if (!config) return;
        config.enabled = enabled;
        await persist();
    }

    async function pause(minutes: number) {
        await pauseTracking(minutes);
        if (config) config.pausedUntilMs = minutes === 0 ? 0 : Date.now() + minutes * 60000;
        await refreshStatus();
    }

    async function confirmWipe() {
        if (!wipeArmed) {
            wipeArmed = true;
            setTimeout(() => (wipeArmed = false), 4000);
            return;
        }
        wipeArmed = false;
        await wipeData();
        await load(dates);
    }

    function updateIdleThreshold(event: Event) {
        if (!config) return;
        const next = Number((event.currentTarget as HTMLInputElement).value);
        config.idleThresholdSecs = Math.max(60, (Number.isFinite(next) && next > 0 ? next : 3) * 60);
    }

    function addExclude() {
        const value = newExclude.trim();
        if (config && value && !config.excludedApps.includes(value)) {
            config.excludedApps = [...config.excludedApps, value];
            newExclude = '';
            void persist();
        }
    }

    function removeExclude(app: string) {
        if (!config) return;
        config.excludedApps = config.excludedApps.filter((entry) => entry !== app);
        void persist();
    }

    function addCategory() {
        if (!config) return;
        config.categories = [
            ...config.categories,
            { name: 'New category', color: '#64748b', kind: 'neutral' } as CategoryDef,
        ];
    }

    function removeCategory(index: number) {
        if (config) config.categories = config.categories.filter((_, itemIndex) => itemIndex !== index);
    }

    function addRule() {
        if (!config) return;
        const category = config.categories[0]?.name ?? 'Other';
        config.rules = [
            ...config.rules,
            { field: 'app', op: 'contains', pattern: '', category } as CategoryRule,
        ];
    }

    function removeRule(index: number) {
        if (config) config.rules = config.rules.filter((_, itemIndex) => itemIndex !== index);
    }

    function addGoal() {
        if (!config) return;
        const category = config.categories[0]?.name ?? 'Development';
        config.goals = [...config.goals, { category, dailyTargetMins: 120 } as Goal];
    }

    function removeGoal(index: number) {
        if (config) config.goals = config.goals.filter((_, itemIndex) => itemIndex !== index);
    }
</script>

<ToolPage
    icon={Clock3}
    iconTint="var(--color-accent)"
    title="Time Tracker"
    description="A private activity journal that stays on this device."
    width="wide"
    fill={false}
>
    {#snippet actions()}
        <Tabs
            tabs={trackerTabs}
            active={tab}
            onChange={(id) => {
                tab = id as TabId;
                timeTrackerTab.set(tab);
            }}
            size="sm"
            ariaLabel="Time Tracker sections"
        />
    {/snippet}

    {#if tab === 'dashboard'}
        <ToolPanel
            padding="lg"
            elevated
            tone={status?.tracking ? 'accent' : 'panel'}
            as="section"
        >
            <div class="now-card" aria-live="polite">
                <div class="now-status">
                    <span
                        class="now-indicator"
                        class:is-tracking={status?.tracking}
                        class:is-paused={status?.paused}
                        aria-hidden="true"
                    >
                        <span></span>
                    </span>
                    <div class="now-copy">
                        <span class="eyebrow">Now</span>
                        <h2>{statusTitle}</h2>
                        <p>{statusDescription}</p>
                    </div>
                </div>

                <div class="now-side">
                    {#if status?.tracking && status.currentCategory}
                        <span class="category-chip">
                            <span
                                class="category-chip-dot"
                                style={'background: ' + colorFor(status.currentCategory) + ';'}
                            ></span>
                            {status.currentCategory}
                        </span>
                    {/if}
                    <div class="now-actions">
                        {#if status?.tracking}
                            <Button variant="secondary" size="sm" icon={Pause} onclick={() => pause(15)}>
                                Pause 15 min
                            </Button>
                            <Button variant="ghost" size="sm" icon={Pause} onclick={() => pause(60)}>
                                1 hour
                            </Button>
                        {:else if status?.paused}
                            <Button variant="primary" size="sm" icon={Play} onclick={() => pause(0)}>
                                Resume tracking
                            </Button>
                        {:else if config}
                            <Button variant="primary" size="sm" icon={Play} onclick={() => setEnabled(true)}>
                                Start tracking
                            </Button>
                        {/if}
                    </div>
                </div>
            </div>
        </ToolPanel>

        <ToolToolbar variant="panel">
            <div class="range-toolbar">
                <div class="range-view">
                    <span class="toolbar-label">View</span>
                    <Tabs
                        tabs={rangeTabs}
                        active={view}
                        onChange={(id) => {
                            view = id as ViewId;
                            timeTrackerView.set(view);
                            selectedSegment = null;
                        }}
                        size="sm"
                        ariaLabel="Activity range"
                    />
                </div>

                <div class="date-navigation">
                    <Button
                        variant="ghost"
                        size="sm"
                        icon={ChevronLeft}
                        iconOnly
                        title={view === 'day' ? 'Previous day' : 'Previous week'}
                        aria-label={view === 'day' ? 'Previous day' : 'Previous week'}
                        onclick={() => shiftDate(-1)}
                    />
                    <div class="range-current">
                        <span>{view === 'day' ? 'Activity for' : 'Week of'}</span>
                        <strong>{rangeLabel}</strong>
                    </div>
                    <Button
                        variant="ghost"
                        size="sm"
                        icon={ChevronRight}
                        iconOnly
                        title={view === 'day' ? 'Next day' : 'Next week'}
                        aria-label={view === 'day' ? 'Next day' : 'Next week'}
                        onclick={() => shiftDate(1)}
                    />
                    {#if !isCurrentRange}
                        <Button variant="secondary" size="sm" onclick={goToday}>Today</Button>
                    {/if}
                    {#if loading}<span class="refresh-state">Updating</span>{/if}
                </div>
            </div>
        </ToolToolbar>

        {#if !summary}
            <ToolPanel padding="lg" as="section">
                <div class="loading-state">
                    <Activity class="loading-icon" />
                    <div>
                        <strong>Loading your activity journal</strong>
                        <span>Preparing the local timeline.</span>
                    </div>
                </div>
            </ToolPanel>
        {:else if summary.totalActiveSecs === 0 && summary.totalIdleSecs === 0}
            <ToolPanel padding="lg" as="section">
                <div class="empty-journal">
                    <span class="empty-journal-icon" aria-hidden="true"><Activity /></span>
                    <div>
                        <h2>No activity yet</h2>
                        {#if config && !config.enabled}
                            <p>Tracking is off. Start it when you want to build a private activity journal.</p>
                            <Button variant="primary" size="sm" icon={Play} onclick={() => setEnabled(true)}>
                                Start tracking
                            </Button>
                        {:else}
                            <p>Your activity will appear here as you use this device.</p>
                        {/if}
                    </div>
                </div>
            </ToolPanel>
        {:else}
            <section class="metric-grid" aria-label="Activity summary">
                <article class="metric-card">
                    <span class="metric-label">Active time</span>
                    <strong>{formatDuration(summary.totalActiveSecs)}</strong>
                    <span class="metric-detail">
                        {view === 'day' ? 'Recorded while you were active' : 'Across this week'}
                    </span>
                </article>
                <article class="metric-card">
                    <span class="metric-label">Away time</span>
                    <strong>{formatDuration(summary.totalIdleSecs)}</strong>
                    <span class="metric-detail">Kept separate from active work</span>
                </article>
                <article class="metric-card metric-card-focus">
                    <div class="metric-focus-head">
                        <span class="metric-label">Focus score</span>
                        <span class="focus-value" style={'color: ' + focusColor + ';'}>
                            {summary.focusScore}%
                        </span>
                    </div>
                    <div class="focus-track" aria-hidden="true">
                        <span
                            style={'width: ' + summary.focusScore + '%; background: ' + focusColor + ';'}
                        ></span>
                    </div>
                    <span class="metric-detail">
                        {topCategory ? 'Most time in ' + topCategory.name : 'Based on productive categories'}
                    </span>
                </article>
            </section>

            <ToolPanel padding="lg" as="section">
                <div class="panel-header">
                    <div>
                        <span class="eyebrow">{view === 'day' ? 'Activity flow' : 'Daily rhythm'}</span>
                        <h2>{view === 'day' ? 'Your day at a glance' : 'How the week unfolded'}</h2>
                        <p>
                            {view === 'day'
                                ? 'Color marks the category of each active block. Away time is muted.'
                                : 'Each bar separates active time from away time.'}
                        </p>
                    </div>
                    <span class="panel-meta">
                        {view === 'day' ? timelineSegs.length + ' blocks' : summary.byDay.length + ' days'}
                    </span>
                </div>

                {#if view === 'day'}
                    <div class="timeline-wrap">
                        <div class="timeline" role="img" aria-label={timelineDescription}>
                            <span class="timeline-rule timeline-rule-quarter" aria-hidden="true"></span>
                            <span class="timeline-rule timeline-rule-half" aria-hidden="true"></span>
                            <span class="timeline-rule timeline-rule-three-quarter" aria-hidden="true"></span>
                            {#each timelineSegs as segment}
                                <button
                                    type="button"
                                    class="timeline-segment"
                                    class:is-idle={segment.idle}
                                    class:is-selected={
                                        selectedSegment?.startMs === segment.startMs &&
                                        selectedSegment.endMs === segment.endMs &&
                                        selectedSegment.app === segment.app
                                    }
                                    style={
                                        'left: ' +
                                        segment.leftPct +
                                        '%; width: ' +
                                        segment.widthPct +
                                        '%; background: ' +
                                        segment.color +
                                        ';'
                                    }
                                    title={'Inspect ' + segment.label}
                                    aria-label={'Inspect ' + segment.label}
                                    aria-pressed={
                                        selectedSegment?.startMs === segment.startMs &&
                                        selectedSegment.endMs === segment.endMs &&
                                        selectedSegment.app === segment.app
                                    }
                                    onclick={() => selectSegment(segment)}
                                ></button>
                            {/each}
                            {#if nowPct >= 0}
                                <span
                                    class="timeline-now"
                                    style={'left: ' + Math.min(100, Math.max(0, nowPct)) + '%;'}
                                    aria-label="Current time"
                                ></span>
                            {/if}
                        </div>
                        <div class="timeline-axis" aria-hidden="true">
                            <span>00:00</span><span>06:00</span><span>12:00</span><span>18:00</span><span>24:00</span>
                        </div>
                        {#if selectedSegment}
                            <div class="activity-inspector" aria-live="polite">
                                <span
                                    class="activity-inspector-icon"
                                    class:is-idle={selectedSegment.idle}
                                    style={
                                        'color: ' +
                                        (selectedSegment.idle
                                            ? '#94a3b8'
                                            : colorFor(selectedSegment.category)) +
                                        ';'
                                    }
                                    aria-hidden="true"
                                >
                                    <Activity />
                                </span>
                                <div class="activity-inspector-copy">
                                    <span class="eyebrow">Selected activity</span>
                                    <strong>
                                        {selectedSegment.idle
                                            ? 'Away from your device'
                                            : humanizeAppName(selectedSegment.app)}
                                    </strong>
                                    <span>
                                        {clockTime(selectedSegment.startMs)} to {clockTime(selectedSegment.endMs)}
                                        | {formatDuration(
                                            Math.floor(
                                                Math.max(0, selectedSegment.endMs - selectedSegment.startMs) /
                                                    1000,
                                            ),
                                        )}
                                        {#if selectedSegment.title}
                                            | {selectedSegment.title}
                                        {/if}
                                    </span>
                                </div>
                                <Button
                                    variant="ghost"
                                    size="sm"
                                    icon={X}
                                    iconOnly
                                    title="Clear selected activity"
                                    aria-label="Clear selected activity"
                                    onclick={() => (selectedSegment = null)}
                                />
                            </div>
                        {/if}
                    </div>
                {:else}
                    <div
                        class="week-chart"
                        role="group"
                        aria-label="Active and away time for each day in the selected week"
                    >
                        {#each summary.byDay as day}
                            {@const total = day.activeSecs + day.idleSecs}
                            <button
                                type="button"
                                class="week-column"
                                onclick={() => openDay(day.date)}
                                title={'Open ' + prettyDate(day.date)}
                                aria-label={
                                    'Open ' +
                                    prettyDate(day.date) +
                                    ', ' +
                                    formatDuration(day.activeSecs) +
                                    ' active'
                                }
                            >
                                <span class="week-value">{formatDuration(day.activeSecs)}</span>
                                <div class="week-bar-area">
                                    <span
                                        class="week-bar-total"
                                        style={'height: ' + percent(total, weekMax) + '%;'}
                                    >
                                        <span
                                            class="week-bar-active"
                                            style={
                                                'height: ' +
                                                (total > 0 ? percent(day.activeSecs, total) : 0) +
                                                '%;'
                                            }
                                        ></span>
                                    </span>
                                </div>
                                <span class:today={day.date === todayKey()} class="week-label">
                                    {weekdayLabel(day.date)}
                                </span>
                            </button>
                        {/each}
                    </div>
                {/if}
            </ToolPanel>

            <div class="insights-grid">
                <ToolPanel padding="lg" as="section">
                    <div class="panel-header panel-header-compact">
                        <div>
                            <span class="eyebrow">Focus mix</span>
                            <h2>Categories</h2>
                        </div>
                        <span class="panel-meta">{summary.byCategory.length} tracked</span>
                    </div>

                    {#if summary.byCategory.length === 0}
                        <p class="quiet-empty">No categorized activity in this range.</p>
                    {:else}
                        <div class="breakdown-list">
                            {#each summary.byCategory as category}
                                {@const CategoryIcon = categoryIconFor(category)}
                                <div class="breakdown-row">
                                    <div class="breakdown-copy">
                                        <span
                                            class="category-icon"
                                            style={
                                                'background: color-mix(in srgb, ' +
                                                category.color +
                                                ' 13%, transparent); color: ' +
                                                category.color +
                                                ';'
                                            }
                                            aria-hidden="true"
                                        ><CategoryIcon class="category-icon-svg" /></span>
                                        <span>{category.name}</span>
                                        <span class="breakdown-time">{formatDuration(category.secs)}</span>
                                    </div>
                                    <div class="breakdown-track" aria-hidden="true">
                                        <span
                                            style={
                                                'width: ' +
                                                percent(category.secs, catMax) +
                                                '%; background: ' +
                                                category.color +
                                                ';'
                                            }
                                        ></span>
                                    </div>
                                </div>
                            {/each}
                        </div>
                    {/if}
                </ToolPanel>

                <ToolPanel padding="lg" as="section">
                    <div class="panel-header panel-header-compact">
                        <div>
                            <span class="eyebrow">Your tools</span>
                            <h2>Top apps</h2>
                        </div>
                        <span class="panel-meta">{topApps.length} shown</span>
                    </div>

                    {#if topApps.length === 0}
                        <p class="quiet-empty">No app activity in this range.</p>
                    {:else}
                        <div class="breakdown-list">
                            {#each topApps as app}
                                {@const appIcon = appIcons[appKey(app.app)]}
                                <div class="breakdown-row">
                                    <div class="breakdown-copy">
                                        <span
                                            class="app-icon"
                                            style={
                                                'color: ' +
                                                sourceColor(app.app) +
                                                '; background: color-mix(in srgb, ' +
                                                sourceColor(app.app) +
                                                ' 13%, transparent);'
                                            }
                                            aria-hidden="true"
                                        >
                                            {#if appIcon}
                                                <img src={appIcon} alt="" />
                                            {:else}
                                                <AppWindow class="app-icon-svg" />
                                            {/if}
                                        </span>
                                        <span>{humanizeAppName(app.app)}</span>
                                        <span class="breakdown-time">{formatDuration(app.secs)}</span>
                                    </div>
                                    <div class="breakdown-track" aria-hidden="true">
                                        <span
                                            style={
                                                'width: ' +
                                                percent(app.secs, appMax) +
                                                '%; background: var(--color-accent);'
                                            }
                                        ></span>
                                    </div>
                                </div>
                            {/each}
                        </div>
                    {/if}
                </ToolPanel>
            </div>

            {#if summary.goals.length > 0}
                <ToolPanel padding="lg" as="section">
                    <div class="panel-header panel-header-compact">
                        <div>
                            <span class="eyebrow">Intent</span>
                            <h2>Goals</h2>
                        </div>
                        <Target class="panel-heading-icon" aria-hidden="true" />
                    </div>

                    <div class="goals-grid">
                        {#each summary.goals as goal}
                            {@const goalPercent = goal.targetMins > 0
                                ? Math.min(100, (goal.actualMins / goal.targetMins) * 100)
                                : 0}
                            <div class="goal-card">
                                <div class="goal-copy">
                                    <strong>{goal.category}</strong>
                                    <span>{goal.actualMins}m of {goal.targetMins}m</span>
                                </div>
                                <span class:complete={goalPercent >= 100} class="goal-percent">
                                    {Math.round(goalPercent)}%
                                </span>
                                <div class="goal-track" aria-hidden="true">
                                    <span
                                        style={
                                            'width: ' +
                                            goalPercent +
                                            '%; background: ' +
                                            (goalPercent >= 100
                                                ? '#10b981'
                                                : colorFor(goal.category)) +
                                            ';'
                                        }
                                    ></span>
                                </div>
                            </div>
                        {/each}
                    </div>
                </ToolPanel>
            {/if}
        {/if}
    {:else if config}
        <ToolToolbar variant="panel">
            <div class="settings-toolbar">
                <div>
                    <span class="toolbar-label">Settings</span>
                    <strong>Local, private, and under your control</strong>
                </div>
                <div class="settings-toolbar-actions">
                    {#if savedTick}<span class="saved-state">Saved</span>{/if}
                    <Button variant="primary" size="sm" icon={ShieldCheck} onclick={persist}>
                        Save changes
                    </Button>
                </div>
            </div>
        </ToolToolbar>

        <div class="settings-stack">
            <ToolPanel padding="lg" as="section">
                <div class="settings-heading">
                    <div>
                        <span class="eyebrow">Capture</span>
                        <h2>Automatic activity tracking</h2>
                        <p>Choose when KeepItLocal records activity and how it recognizes away time.</p>
                    </div>
                </div>

                <div class="settings-list">
                    <div class="setting-row">
                        <div>
                            <strong>Tracking enabled</strong>
                            <span>Turn all activity capture on or off.</span>
                        </div>
                        <Toggle
                            checked={config.enabled}
                            onchange={(enabled) => void setEnabled(enabled)}
                            ariaLabel="Enable time tracking"
                        />
                    </div>
                    <div class="setting-row setting-row-input">
                        <div>
                            <strong>Away threshold</strong>
                            <span>Mark time as away after this many inactive minutes.</span>
                        </div>
                        <label class="number-field">
                            <input
                                type="number"
                                min="1"
                                max="120"
                                value={idleMinutes}
                                oninput={updateIdleThreshold}
                            />
                            <span>minutes</span>
                        </label>
                    </div>
                </div>
            </ToolPanel>

            <ToolPanel padding="lg" as="section">
                <div class="settings-heading">
                    <div>
                        <span class="eyebrow">Privacy</span>
                        <h2>Keep sensitive activity private</h2>
                        <p>Everything remains local. Excluded apps are retained without their window titles.</p>
                    </div>
                    <EyeOff class="settings-heading-icon" aria-hidden="true" />
                </div>

                <div class="settings-list">
                    <div class="setting-row">
                        <div>
                            <strong>Capture window titles</strong>
                            <span>Keep richer context in reports. Turn it off to store app names only.</span>
                        </div>
                        <Toggle
                            checked={config.captureTitles}
                            onchange={(enabled) => (config ? (config.captureTitles = enabled) : undefined)}
                            ariaLabel="Capture window titles"
                        />
                    </div>
                    <div class="setting-row setting-row-input">
                        <div>
                            <strong>Keep history</strong>
                            <span>Set zero to retain activity until you remove it.</span>
                        </div>
                        <label class="number-field">
                            <input type="number" min="0" max="3650" bind:value={config.retentionDays} />
                            <span>days</span>
                        </label>
                    </div>
                </div>

                <div class="exclude-area">
                    <div class="field-copy">
                        <strong>Excluded apps</strong>
                        <span>For example, a password manager or banking app.</span>
                    </div>
                    <div class="exclude-chips">
                        {#each config.excludedApps as app}
                            <span class="exclude-chip">
                                {app}
                                <Button
                                    variant="ghost"
                                    size="sm"
                                    icon={X}
                                    iconOnly
                                    title={'Remove ' + app}
                                    aria-label={'Remove ' + app}
                                    onclick={() => removeExclude(app)}
                                />
                            </span>
                        {/each}
                        {#if config.excludedApps.length === 0}
                            <span class="quiet-empty">No excluded apps yet.</span>
                        {/if}
                    </div>
                    <div class="exclude-form">
                        <div class="exclude-input">
                            <TextInput
                                size="sm"
                                bind:value={newExclude}
                                placeholder="Add an app, for example KeePass"
                                onkeydown={(event) => event.key === 'Enter' && addExclude()}
                                aria-label="Add excluded app"
                            />
                        </div>
                        <Button variant="secondary" size="sm" icon={Plus} onclick={addExclude}>Add</Button>
                    </div>
                </div>
            </ToolPanel>

            <div class="organization-heading">
                <div>
                    <span class="eyebrow">Organization</span>
                    <h2>Make the journal meaningful</h2>
                    <p>Set categories, classification rules, and daily targets.</p>
                </div>
            </div>

            <ToolPanel padding="lg" as="section">
                <div class="panel-header panel-header-compact">
                    <div>
                        <h2>Categories</h2>
                        <p>Each category gives activity a color and a focus meaning.</p>
                    </div>
                    <Button variant="secondary" size="sm" icon={Plus} onclick={addCategory}>Add category</Button>
                </div>

                <div class="config-list">
                    {#each config.categories as category, index}
                        <div class="category-editor">
                            <input
                                class="color-input"
                                type="color"
                                bind:value={category.color}
                                aria-label={'Color for ' + category.name}
                            />
                            <div class="category-name-input">
                                <TextInput
                                    size="sm"
                                    bind:value={category.name}
                                    aria-label="Category name"
                                />
                            </div>
                            <div class="category-kind-select">
                                <Select
                                    size="sm"
                                    bind:value={category.kind}
                                    options={categoryKindOptions}
                                    aria-label={'Kind for ' + category.name}
                                />
                            </div>
                            <Button
                                variant="ghost"
                                size="sm"
                                icon={X}
                                iconOnly
                                title={'Remove ' + category.name}
                                aria-label={'Remove ' + category.name}
                                onclick={() => removeCategory(index)}
                            />
                        </div>
                    {/each}
                </div>
            </ToolPanel>

            <ToolPanel padding="lg" as="section">
                <div class="panel-header panel-header-compact">
                    <div>
                        <h2>Classification rules</h2>
                        <p>Custom rules run from top to bottom and override built-in categories.</p>
                    </div>
                    <Button variant="secondary" size="sm" icon={Plus} onclick={addRule}>Add rule</Button>
                </div>

                {#if config.rules.length === 0}
                    <p class="quiet-empty">No custom rules. Built-in categories still apply.</p>
                {:else}
                    <div class="config-list">
                        {#each config.rules as rule, index}
                            <div class="rule-editor">
                                <div class="rule-select">
                                    <Select
                                        size="sm"
                                        bind:value={rule.field}
                                        options={ruleFieldOptions}
                                        aria-label="Rule field"
                                    />
                                </div>
                                <div class="rule-select">
                                    <Select
                                        size="sm"
                                        bind:value={rule.op}
                                        options={ruleOperatorOptions}
                                        aria-label="Rule operator"
                                    />
                                </div>
                                <div class="rule-pattern">
                                    <TextInput
                                        size="sm"
                                        bind:value={rule.pattern}
                                        placeholder="Match text"
                                        aria-label="Rule match text"
                                    />
                                </div>
                                <span class="rule-arrow" aria-hidden="true">to</span>
                                <div class="rule-category">
                                    <Select
                                        size="sm"
                                        bind:value={rule.category}
                                        options={categoryOptions}
                                        aria-label="Rule category"
                                    />
                                </div>
                                <Button
                                    variant="ghost"
                                    size="sm"
                                    icon={X}
                                    iconOnly
                                    title="Remove rule"
                                    aria-label="Remove rule"
                                    onclick={() => removeRule(index)}
                                />
                            </div>
                        {/each}
                    </div>
                {/if}
            </ToolPanel>

            <ToolPanel padding="lg" as="section">
                <div class="panel-header panel-header-compact">
                    <div>
                        <h2>Daily goals</h2>
                        <p>Set a daily target for the categories that matter most.</p>
                    </div>
                    <Button variant="secondary" size="sm" icon={Plus} onclick={addGoal}>Add goal</Button>
                </div>

                {#if config.goals.length === 0}
                    <p class="quiet-empty">No daily goals yet.</p>
                {:else}
                    <div class="config-list">
                        {#each config.goals as goal, index}
                            <div class="goal-editor">
                                <div class="goal-category-select">
                                    <Select
                                        size="sm"
                                        bind:value={goal.category}
                                        options={categoryOptions}
                                        aria-label="Goal category"
                                    />
                                </div>
                                <label class="goal-number">
                                    <input type="number" min="0" bind:value={goal.dailyTargetMins} />
                                    <span>minutes per day</span>
                                </label>
                                <Button
                                    variant="ghost"
                                    size="sm"
                                    icon={X}
                                    iconOnly
                                    title="Remove goal"
                                    aria-label="Remove goal"
                                    onclick={() => removeGoal(index)}
                                />
                            </div>
                        {/each}
                    </div>
                {/if}
            </ToolPanel>

            <ToolPanel padding="lg" as="section">
                <div class="danger-zone">
                    <div>
                        <span class="eyebrow">Danger zone</span>
                        <h2>Wipe tracked activity</h2>
                        <p>Deletes recorded activity only. Your categories, rules, and goals remain.</p>
                    </div>
                    <Button
                        variant="danger"
                        size="sm"
                        icon={Trash2}
                        onclick={confirmWipe}
                    >
                        {wipeArmed ? 'Click again to confirm' : 'Wipe activity'}
                    </Button>
                </div>
            </ToolPanel>
        </div>
    {:else}
        <ToolPanel padding="lg" as="section">
            <div class="loading-state">
                <Settings class="loading-icon" />
                <div>
                    <strong>Loading tracker settings</strong>
                    <span>Reading your local configuration.</span>
                </div>
            </div>
        </ToolPanel>
    {/if}
</ToolPage>

<style>
    .now-card,
    .range-toolbar,
    .settings-toolbar,
    .panel-header,
    .setting-row,
    .danger-zone {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 16px;
    }

    .now-status,
    .now-side,
    .now-actions,
    .range-view,
    .date-navigation,
    .settings-toolbar-actions,
    .metric-focus-head,
    .breakdown-copy,
    .goal-copy,
    .exclude-form,
    .category-editor,
    .rule-editor,
    .goal-editor {
        display: flex;
        align-items: center;
    }

    .now-status {
        gap: 14px;
        min-width: 0;
    }

    .now-indicator {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 42px;
        height: 42px;
        flex: none;
    }

    .now-indicator span {
        width: 10px;
        height: 10px;
        border-radius: 50%;
        background: var(--color-muted);
    }

    .now-indicator.is-tracking span {
        background: #22c55e;
        box-shadow: 0 0 0 5px color-mix(in srgb, #22c55e 16%, transparent);
        animation: tracker-recording-pulse 1.8s ease-out infinite;
    }

    .now-indicator.is-paused {
        border-color: color-mix(in srgb, #f59e0b 42%, var(--color-border));
        background: color-mix(in srgb, #f59e0b 12%, transparent);
    }

    .now-indicator.is-paused span {
        background: #f59e0b;
    }

    .now-copy {
        min-width: 0;
    }

    .eyebrow,
    .toolbar-label {
        display: block;
        color: var(--color-muted);
        font-size: 10px;
        font-weight: 700;
        letter-spacing: 0.08em;
        text-transform: uppercase;
    }

    .now-copy h2,
    .panel-header h2,
    .settings-heading h2,
    .organization-heading h2,
    .empty-journal h2,
    .danger-zone h2 {
        margin: 3px 0 0;
        color: var(--color-text);
        font-size: 18px;
        font-weight: 650;
        letter-spacing: -0.014em;
        line-height: 1.25;
    }

    .now-copy p,
    .panel-header p,
    .settings-heading p,
    .organization-heading p,
    .empty-journal p,
    .danger-zone p {
        max-width: 68ch;
        margin: 4px 0 0;
        color: var(--color-text-secondary);
        font-size: 12.5px;
        line-height: 1.48;
    }

    .now-side {
        justify-content: flex-end;
        gap: 10px;
        flex-wrap: wrap;
        flex: none;
    }

    .now-actions {
        gap: 6px;
        flex-wrap: wrap;
    }

    .category-chip,
    .panel-meta,
    .saved-state,
    .refresh-state {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        min-height: 24px;
        padding: 0 9px;
        border: 1px solid var(--color-border);
        border-radius: 999px;
        color: var(--color-text-secondary);
        background: var(--color-panel-2);
        font-size: 11px;
        font-weight: 600;
        white-space: nowrap;
    }

    .category-chip-dot {
        display: inline-block;
        flex: none;
        border-radius: 50%;
    }

    .category-chip-dot {
        width: 7px;
        height: 7px;
    }

    .range-toolbar {
        width: 100%;
    }

    .range-view,
    .date-navigation {
        gap: 8px;
    }

    .range-current {
        min-width: 170px;
        text-align: center;
    }

    .range-current span {
        display: block;
        color: var(--color-muted);
        font-size: 10px;
        font-weight: 600;
        letter-spacing: 0.05em;
        text-transform: uppercase;
    }

    .range-current strong {
        display: block;
        margin-top: 1px;
        color: var(--color-text);
        font-size: 12.5px;
        font-weight: 600;
        line-height: 1.25;
    }

    .refresh-state {
        border-color: color-mix(in srgb, var(--color-accent) 30%, var(--color-border));
        color: var(--color-accent);
    }

    .loading-state,
    .empty-journal {
        display: flex;
        align-items: center;
        justify-content: center;
        gap: 14px;
        min-height: 180px;
        color: var(--color-text-secondary);
        text-align: center;
    }

    .loading-state {
        min-height: 120px;
    }

    .loading-state > div {
        display: flex;
        flex-direction: column;
        gap: 3px;
        text-align: left;
    }

    .loading-state strong {
        color: var(--color-text);
        font-size: 13px;
    }

    .loading-state span {
        font-size: 12px;
    }

    :global(.loading-icon),
    .empty-journal-icon {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 38px;
        height: 38px;
        flex: none;
        border-radius: 10px;
        color: var(--color-accent);
        background: color-mix(in srgb, var(--color-accent) 12%, transparent);
    }

    .loading-state :global(.loading-icon),
    .empty-journal-icon :global(svg) {
        width: 18px;
        height: 18px;
    }

    .empty-journal {
        justify-content: flex-start;
        text-align: left;
    }

    .empty-journal > div {
        max-width: 460px;
    }

    .empty-journal :global(.btn) {
        margin-top: 12px;
    }

    .metric-grid {
        display: grid;
        grid-template-columns: repeat(3, minmax(0, 1fr));
        gap: 10px;
    }

    .metric-card {
        display: flex;
        flex-direction: column;
        justify-content: center;
        min-height: 118px;
        padding: 16px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card);
        background: var(--color-panel);
    }

    .metric-label {
        color: var(--color-muted);
        font-size: 10px;
        font-weight: 700;
        letter-spacing: 0.08em;
        text-transform: uppercase;
    }

    .metric-card > strong {
        margin-top: 6px;
        color: var(--color-text);
        font-size: 25px;
        font-weight: 650;
        letter-spacing: -0.03em;
        line-height: 1;
        font-variant-numeric: tabular-nums;
    }

    .metric-detail {
        margin-top: 8px;
        color: var(--color-text-secondary);
        font-size: 11.5px;
        line-height: 1.35;
    }

    .metric-focus-head {
        justify-content: space-between;
        gap: 12px;
    }

    .focus-value {
        font-size: 20px;
        font-weight: 700;
        letter-spacing: -0.025em;
        font-variant-numeric: tabular-nums;
    }

    .focus-track,
    .breakdown-track,
    .goal-track {
        overflow: hidden;
        height: 5px;
        border-radius: 999px;
        background: var(--color-panel-3);
    }

    .focus-track {
        margin-top: 14px;
    }

    .focus-track span,
    .breakdown-track span,
    .goal-track span {
        display: block;
        height: 100%;
        border-radius: inherit;
    }

    .panel-header {
        align-items: flex-start;
        margin-bottom: 18px;
    }

    .panel-header-compact {
        margin-bottom: 14px;
    }

    .panel-header-compact h2 {
        margin-top: 2px;
        font-size: 16px;
    }

    .panel-header-compact p {
        margin-top: 3px;
    }

    :global(.panel-heading-icon),
    :global(.settings-heading-icon) {
        width: 18px;
        height: 18px;
        flex: none;
        color: var(--color-muted);
    }

    .timeline {
        position: relative;
        height: 64px;
        overflow: hidden;
        border: 1px solid var(--color-border);
        border-radius: 10px;
        background: var(--color-panel-2);
    }

    .timeline-rule,
    .timeline-segment,
    .timeline-now {
        position: absolute;
        top: 0;
        height: 100%;
    }

    .timeline-rule {
        z-index: 1;
        width: 1px;
        background: color-mix(in srgb, var(--color-border) 76%, transparent);
    }

    .timeline-rule-quarter {
        left: 25%;
    }

    .timeline-rule-half {
        left: 50%;
    }

    .timeline-rule-three-quarter {
        left: 75%;
    }

    .timeline-segment {
        z-index: 2;
        min-width: 1px;
        padding: 0;
        border: 0;
        opacity: 0.88;
        cursor: pointer;
        transition:
            filter var(--dur-micro, 130ms) var(--ease-out, ease),
            opacity var(--dur-micro, 130ms) var(--ease-out, ease);
    }

    .timeline-segment.is-idle {
        opacity: 0.5;
    }

    .timeline-segment:hover,
    .timeline-segment.is-selected {
        filter: brightness(1.18);
        opacity: 1;
    }

    .timeline-segment.is-selected {
        box-shadow:
            inset 0 0 0 2px color-mix(in srgb, var(--color-text) 80%, transparent),
            0 0 0 1px color-mix(in srgb, var(--color-bg) 88%, transparent);
    }

    .timeline-segment:focus-visible {
        z-index: 4;
        outline: 2px solid var(--color-accent);
        outline-offset: -2px;
    }

    .timeline-now {
        z-index: 3;
        width: 1px;
        background: var(--color-text);
        box-shadow: 0 0 0 1px color-mix(in srgb, var(--color-panel) 72%, transparent);
    }

    .timeline-axis {
        display: grid;
        grid-template-columns: repeat(5, 1fr);
        margin-top: 7px;
        color: var(--color-muted);
        font-size: 10px;
        font-variant-numeric: tabular-nums;
    }

    .timeline-axis span:nth-child(2),
    .timeline-axis span:nth-child(3),
    .timeline-axis span:nth-child(4) {
        text-align: center;
    }

    .timeline-axis span:last-child {
        text-align: right;
    }

    .activity-inspector {
        display: flex;
        align-items: center;
        gap: 10px;
        margin-top: 14px;
        padding: 10px;
        border: 1px solid color-mix(in srgb, var(--color-accent) 28%, var(--color-border));
        border-radius: 10px;
        background: color-mix(in srgb, var(--color-accent) 6%, var(--color-panel-2));
    }

    .activity-inspector-icon,
    .category-icon,
    .app-icon {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        flex: none;
        overflow: hidden;
    }

    .activity-inspector-icon {
        width: 30px;
        height: 30px;
        border-radius: 8px;
        background: color-mix(in srgb, currentColor 14%, transparent);
    }

    .activity-inspector-icon.is-idle {
        background: color-mix(in srgb, #94a3b8 14%, transparent);
    }

    .activity-inspector-icon :global(svg) {
        width: 15px;
        height: 15px;
    }

    .activity-inspector-copy {
        display: flex;
        flex: 1;
        flex-direction: column;
        gap: 2px;
        min-width: 0;
    }

    .activity-inspector-copy strong {
        overflow: hidden;
        color: var(--color-text);
        font-size: 12.5px;
        font-weight: 650;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .activity-inspector-copy > span:last-child {
        overflow: hidden;
        color: var(--color-text-secondary);
        font-size: 11.5px;
        line-height: 1.35;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .week-chart {
        display: grid;
        grid-template-columns: repeat(7, minmax(0, 1fr));
        gap: 9px;
        min-height: 230px;
    }

    .week-column {
        display: flex;
        flex-direction: column;
        align-items: center;
        min-width: 0;
        gap: 7px;
        padding: 5px 4px;
        border: 1px solid transparent;
        border-radius: 8px;
        color: inherit;
        background: transparent;
        cursor: pointer;
        transition:
            background-color var(--dur-micro, 130ms) var(--ease-out, ease),
            border-color var(--dur-micro, 130ms) var(--ease-out, ease);
    }

    .week-column:hover {
        border-color: color-mix(in srgb, var(--color-accent) 26%, var(--color-border));
        background: color-mix(in srgb, var(--color-accent) 7%, transparent);
    }

    .week-column:focus-visible {
        outline: none;
        border-color: var(--color-accent);
        box-shadow: 0 0 0 3px color-mix(in srgb, var(--color-accent) 20%, transparent);
    }

    .week-value,
    .week-label {
        color: var(--color-muted);
        font-size: 10px;
        font-variant-numeric: tabular-nums;
    }

    .week-bar-area {
        display: flex;
        align-items: flex-end;
        width: 100%;
        flex: 1;
        min-height: 160px;
    }

    .week-bar-total {
        display: block;
        position: relative;
        width: 100%;
        min-height: 4px;
        overflow: hidden;
        border-radius: 7px 7px 3px 3px;
        background: var(--color-panel-3);
    }

    .week-bar-active {
        display: block;
        position: absolute;
        bottom: 0;
        width: 100%;
        border-radius: 7px 7px 3px 3px;
        background: var(--color-accent);
    }

    .week-label.today {
        color: var(--color-text);
        font-weight: 700;
    }

    .insights-grid {
        display: grid;
        grid-template-columns: repeat(2, minmax(0, 1fr));
        gap: 12px;
    }

    .breakdown-list {
        display: flex;
        flex-direction: column;
        gap: 13px;
    }

    .breakdown-row {
        min-width: 0;
    }

    .breakdown-copy {
        gap: 7px;
        min-width: 0;
        margin-bottom: 6px;
        color: var(--color-text);
        font-size: 12.5px;
        line-height: 1.2;
    }

    .breakdown-copy > span:nth-child(2) {
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .category-icon,
    .app-icon {
        width: 25px;
        height: 25px;
        border-radius: 7px;
    }

    .category-icon :global(.category-icon-svg),
    .app-icon :global(.app-icon-svg) {
        width: 14px;
        height: 14px;
    }

    .app-icon img {
        display: block;
        width: 100%;
        height: 100%;
        object-fit: contain;
    }

    .breakdown-time {
        margin-left: auto;
        color: var(--color-muted);
        font-size: 11.5px;
        font-variant-numeric: tabular-nums;
    }

    .quiet-empty {
        margin: 0;
        color: var(--color-muted);
        font-size: 12px;
        line-height: 1.45;
    }

    .goals-grid {
        display: grid;
        grid-template-columns: repeat(auto-fit, minmax(210px, 1fr));
        gap: 10px;
    }

    .goal-card {
        padding: 13px;
        border: 1px solid var(--color-border);
        border-radius: 10px;
        background: var(--color-panel-2);
    }

    .goal-copy {
        justify-content: space-between;
        gap: 10px;
    }

    .goal-copy strong {
        color: var(--color-text);
        font-size: 12.5px;
    }

    .goal-copy span {
        color: var(--color-muted);
        font-size: 11px;
        font-variant-numeric: tabular-nums;
    }

    .goal-percent {
        display: block;
        margin-top: 6px;
        color: var(--color-text-secondary);
        font-size: 11px;
        font-weight: 700;
        font-variant-numeric: tabular-nums;
    }

    .goal-percent.complete {
        color: #10b981;
    }

    .goal-track {
        margin-top: 9px;
    }

    .settings-stack {
        display: flex;
        flex-direction: column;
        gap: 12px;
    }

    .settings-toolbar {
        width: 100%;
    }

    .settings-toolbar strong {
        display: block;
        margin-top: 2px;
        color: var(--color-text);
        font-size: 13px;
        font-weight: 600;
    }

    .settings-toolbar-actions {
        gap: 8px;
        flex: none;
    }

    .saved-state {
        color: var(--color-accent);
        border-color: color-mix(in srgb, var(--color-accent) 32%, var(--color-border));
        background: color-mix(in srgb, var(--color-accent) 10%, transparent);
    }

    .settings-heading,
    .organization-heading {
        display: flex;
        align-items: flex-start;
        justify-content: space-between;
        gap: 16px;
        margin-bottom: 18px;
    }

    .organization-heading {
        padding: 7px 4px 1px;
    }

    .settings-list {
        display: flex;
        flex-direction: column;
        border-top: 1px solid color-mix(in srgb, var(--color-border) 72%, transparent);
    }

    .setting-row {
        min-height: 68px;
        padding: 14px 0;
        border-bottom: 1px solid color-mix(in srgb, var(--color-border) 72%, transparent);
    }

    .setting-row > div {
        display: flex;
        flex-direction: column;
        gap: 3px;
        min-width: 0;
    }

    .setting-row strong,
    .field-copy strong {
        color: var(--color-text);
        font-size: 13px;
        font-weight: 600;
    }

    .setting-row span,
    .field-copy span {
        color: var(--color-text-secondary);
        font-size: 12px;
        line-height: 1.42;
    }

    .number-field,
    .goal-number {
        display: inline-flex;
        align-items: center;
        gap: 8px;
        flex: none;
        color: var(--color-muted);
        font-size: 11.5px;
        white-space: nowrap;
    }

    .number-field input,
    .goal-number input {
        width: 72px;
        height: 30px;
        padding: 0 8px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        color: var(--color-text);
        background: var(--color-panel-2);
        font-size: 12px;
        font-variant-numeric: tabular-nums;
    }

    .number-field input:focus-visible,
    .goal-number input:focus-visible,
    .color-input:focus-visible {
        outline: none;
        border-color: var(--color-accent);
        box-shadow: 0 0 0 3px color-mix(in srgb, var(--color-accent) 22%, transparent);
    }

    .exclude-area {
        margin-top: 18px;
    }

    .field-copy {
        display: flex;
        flex-direction: column;
        gap: 3px;
    }

    .exclude-chips {
        display: flex;
        flex-wrap: wrap;
        gap: 7px;
        margin-top: 11px;
    }

    .exclude-chip {
        display: inline-flex;
        align-items: center;
        gap: 1px;
        min-height: 28px;
        padding: 0 3px 0 9px;
        border: 1px solid var(--color-border);
        border-radius: 999px;
        color: var(--color-text);
        background: var(--color-panel-2);
        font-size: 12px;
    }

    .exclude-chip :global(.btn) {
        margin-left: 1px;
    }

    .exclude-form {
        gap: 8px;
        margin-top: 12px;
    }

    .exclude-input,
    .category-name-input,
    .rule-pattern,
    .rule-category,
    .goal-category-select {
        min-width: 0;
        flex: 1;
    }

    .config-list {
        display: flex;
        flex-direction: column;
        gap: 8px;
    }

    .category-editor,
    .rule-editor,
    .goal-editor {
        gap: 8px;
        min-width: 0;
        padding: 8px;
        border: 1px solid var(--color-border);
        border-radius: 10px;
        background: var(--color-panel-2);
    }

    .color-input {
        width: 30px;
        height: 30px;
        flex: none;
        padding: 2px;
        border: 1px solid var(--color-border);
        border-radius: 7px;
        background: var(--color-panel);
        cursor: pointer;
    }

    .category-kind-select {
        width: 148px;
        flex: none;
    }

    .rule-select {
        width: 120px;
        flex: none;
    }

    .rule-arrow {
        flex: none;
        color: var(--color-muted);
        font-size: 11px;
        font-weight: 600;
        text-transform: uppercase;
    }

    .goal-number {
        width: 166px;
        justify-content: flex-end;
    }

    .goal-number input {
        width: 68px;
    }

    .danger-zone {
        align-items: flex-end;
        padding: 2px;
    }

    .danger-zone h2 {
        color: var(--color-error, #ef4444);
    }

    @keyframes tracker-recording-pulse {
        0%,
        100% {
            box-shadow: 0 0 0 5px color-mix(in srgb, #22c55e 16%, transparent);
        }
        50% {
            box-shadow: 0 0 0 9px color-mix(in srgb, #22c55e 0%, transparent);
        }
    }

    @media (prefers-reduced-motion: reduce) {
        .now-indicator.is-tracking span {
            animation: none;
        }
    }

    @media (max-width: 820px) {
        .metric-grid,
        .insights-grid {
            grid-template-columns: 1fr;
        }

        .now-card,
        .range-toolbar {
            align-items: flex-start;
            flex-direction: column;
        }

        .now-side {
            justify-content: flex-start;
        }

        .range-toolbar {
            width: auto;
        }

        .date-navigation {
            flex-wrap: wrap;
        }

        .rule-editor {
            display: grid;
            grid-template-columns: repeat(2, minmax(0, 1fr)) auto;
        }

        .rule-select,
        .rule-pattern,
        .rule-category {
            width: auto;
        }

        .rule-arrow {
            display: none;
        }

        .rule-pattern,
        .rule-category {
            grid-column: span 1;
        }
    }

    @media (max-width: 640px) {
        .now-card,
        .settings-toolbar,
        .panel-header,
        .setting-row,
        .danger-zone {
            align-items: flex-start;
            flex-direction: column;
        }

        .now-side,
        .settings-toolbar-actions {
            align-items: flex-start;
        }

        .range-current {
            min-width: 0;
            text-align: left;
        }

        .date-navigation {
            width: 100%;
        }

        .metric-grid {
            grid-template-columns: 1fr;
        }

        .week-chart {
            gap: 5px;
        }

        .week-bar-area {
            min-height: 126px;
        }

        .activity-inspector {
            align-items: flex-start;
        }

        .exclude-form {
            align-items: stretch;
            flex-direction: column;
        }

        .category-editor,
        .goal-editor {
            align-items: stretch;
            flex-wrap: wrap;
        }

        .category-name-input {
            min-width: calc(100% - 46px);
        }

        .category-kind-select,
        .goal-number {
            width: auto;
            flex: 1;
        }

        .rule-editor {
            grid-template-columns: 1fr;
        }

        .rule-pattern,
        .rule-category {
            grid-column: auto;
        }

        .goal-number {
            justify-content: flex-start;
        }
    }
</style>
