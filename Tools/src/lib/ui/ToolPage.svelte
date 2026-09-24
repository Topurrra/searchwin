<script lang="ts">
    /*
      ToolPage — the calm replacement for the radial-gradient hero
      pattern (the one replicated in 49 tool files). Renders a
      macOS-style page header (icon AND title on the same line,
      description flowing underneath aligned with the title's left
      edge) and a content slot below.

      Design intent:
        - The HEADER is small and stays out of the way; the WORK
          AREA is the focus. Inverse of the old hero+stats-tiles era.
        - macOS Settings / Finder convention: the app icon sits next
          to the title on one line; the description (subtitle) flows
          under the title aligned to the title's left edge — NOT
          under the icon. This reads as "title is the lead, icon is
          the identifier" rather than "icon is the lead".
        - Three text sizes only: title (22), description (13), meta
          (11.5). Anything else needs a strong reason.
        - The header icon uses the tool's *pack color* as a tint on
          its tile background — accent identity stays reserved for
          actions and active states (DesignPro §color).
        - A footer slot exists for "keyboard hints" strips, deferred
          until needed; if empty, the strip doesn't render.

      Layout (macOS pattern):
        ┌────────────────────────────────────────────────────────┐
        │ [icon] Title                              [actions]    │   row 1
        │        Description text aligns under the title         │   row 2
        ├────────────────────────────────────────────────────────┤
        │                                                        │
        │  children (toolbar / panel / list / etc.)              │
        │                                                        │
        ├────────────────────────────────────────────────────────┤
        │ [footer — keyboard hints strip, optional]              │
        └────────────────────────────────────────────────────────┘
    */
    import type { Snippet } from 'svelte';
    import { getContext } from 'svelte';

    interface Props {
        /** Lucide icon component for the tool. */
        icon?: any;
        /** Pack color tint for the icon tile — pass a CSS color value
         *  (e.g. `var(--pack-pdf)` or `#a78bfa`). Defaults to the
         *  global accent if omitted. */
        iconTint?: string;
        title: string;
        /** One-line description. Long descriptions go in the body. */
        description?: string;
        /** Right-aligned snippet — primary CTA + secondary buttons.
         *  Aim for one primary + up to two secondary; more = move some
         *  to a "More" overflow menu. */
        actions?: Snippet;
        /** Optional footer strip, e.g. keyboard hint chips. */
        footer?: Snippet;
        /** Body content. */
        children?: Snippet;
        /** Max content width — `wide` (1280px default), `medium`
         *  (960px), or `narrow` (720px). */
        width?: 'wide' | 'medium' | 'narrow';
        /** When true, the body becomes a `flex: 1` column so children
         *  with `is-scroll` (ToolPanel) fill the page. Most tool pages
         *  want this; turn off for short, content-driven pages. */
        fill?: boolean;
    }

    let {
        icon: Icon,
        iconTint,
        title,
        description = '',
        actions,
        footer,
        children,
        width = 'wide',
        fill = true,
    }: Props = $props();

    /* When rendered inside a Category Workspace, the workspace owns the
       compact pack context and active tool title. ToolPage therefore keeps
       only its description and primary actions, avoiding a duplicate header.
       Detected via context the workspace sets — tools themselves need no
       change. */
    const inCategory = getContext('kil:category-workspace') === true;
</script>

<section class="tool-page width-{width} {fill ? 'is-fill' : ''}" class:is-embedded={inCategory}>
    {#if inCategory}
        <!-- Embedded in a Category Workspace: its compact header owns the
             title, so surface only the description + actions (and skip the
             bar entirely when there's neither). -->
        {#if description || actions}
            <div class="tp-embedded-bar">
                {#if description}<p class="tp-desc tp-desc-embedded">{description}</p>{/if}
                {#if actions}<div class="tp-actions">{@render actions()}</div>{/if}
            </div>
        {/if}
    {:else}
        <!-- Header uses a 3-column grid so the description (row 2) can
             start at the title column's left edge — under the title, not
             under the icon. The icon spans column 1 only on row 1; row
             2's column 1 is empty so the description flows directly
             beneath the title text. -->
        <header class="tp-header" class:has-icon={Boolean(Icon)}>
            {#if Icon}
                <span
                    class="tp-icon"
                    style={iconTint
                        ? `color: ${iconTint};`
                        : ''}
                    aria-hidden="true"
                >
                    <Icon class="tp-icon-svg" />
                </span>
            {/if}
            <h1 class="tp-title">{title}</h1>
            {#if actions}
                <div class="tp-actions">{@render actions()}</div>
            {/if}
            {#if description}
                <p class="tp-desc">{description}</p>
            {/if}
        </header>
    {/if}

    <div class="tp-body">
        {@render children?.()}
    </div>

    {#if footer}
        <footer class="tp-footer">{@render footer()}</footer>
    {/if}
</section>

<style>
    .tool-page {
        display: flex;
        flex-direction: column;
        gap: 16px;
        margin: 0 auto;
        padding: 28px 32px 32px;
        width: 100%;
        height: 100%;
    }
    .width-wide {
        max-width: 1280px;
    }
    .width-medium {
        max-width: 960px;
    }
    .width-narrow {
        max-width: 720px;
    }
    .tool-page.is-fill .tp-body {
        flex: 1;
        min-height: 0;
        display: flex;
        flex-direction: column;
        gap: 12px;
    }

    /* Embedded in a Category Workspace — the workspace owns the outer
       padding / width / page header, so ToolPage collapses to just its
       content (slim action bar + body). */
    .tool-page.is-embedded {
        padding: 0;
        max-width: none;
        height: auto;
        gap: 12px;
    }
    .tp-embedded-bar {
        display: flex;
        align-items: center;
        gap: 16px;
    }
    .tp-desc-embedded {
        flex: 1;
        min-width: 0;
    }
    /* When there's no description, push the actions to the right. */
    .tp-embedded-bar .tp-actions {
        margin-left: auto;
    }

    /* ─── Header ──────────────────────────────────────────────────
       3-column grid: [icon] [title + description] [actions].
       Row 1 carries icon + title + actions. Row 2 (the description)
       starts at the title's left edge — column 2 — so it flows
       directly beneath the title rather than under the icon.

       When there is NO icon the grid collapses to 2 columns
       (title-area + actions), and the description still aligns
       to row 2 of the title column. */
    .tp-header {
        display: grid;
        grid-template-columns: 1fr auto;
        grid-template-areas:
            'title actions'
            'desc  desc';
        align-items: center;
        column-gap: 16px;
        row-gap: 4px;
    }
    .tp-header.has-icon {
        grid-template-columns: auto 1fr auto;
        grid-template-areas:
            'icon  title actions'
            '.     desc  desc';
    }
    /* Header icon remains aligned with the title without a tile background. */
    .tp-icon {
        grid-area: icon;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 40px;
        height: 40px;
        border-radius: 10px;
        /* background: var(--color-accent-soft); */
        color: var(--color-accent);
        box-shadow:
            inset 0 1px 0 color-mix(in srgb, var(--color-text) 10%, transparent),
            inset 0 0 0 1px color-mix(in srgb, var(--color-text) 5%, transparent);
    }
    .tool-page :global(.tp-icon-svg) {
        width: 20px;
        height: 20px;
    }
    .tp-title {
        grid-area: title;
        margin: 0;
        font-size: 22px;
        font-weight: 600;
        line-height: 1.2;
        letter-spacing: -0.018em;
        color: var(--color-text);
        text-wrap: balance;
        align-self: center;
    }
    .tp-desc {
        grid-area: desc;
        margin: 0;
        font-size: 13px;
        line-height: 1.45;
        color: var(--color-text-secondary);
        text-wrap: pretty;
        max-width: 72ch;
    }
    .tp-actions {
        grid-area: actions;
        display: flex;
        align-items: center;
        gap: 8px;
        align-self: center;
    }

    /* ─── Body ────────────────────────────────────────────────── */
    .tp-body {
        display: flex;
        flex-direction: column;
        gap: 12px;
    }

    /* ─── Footer (keyboard hints strip) ───────────────────────── */
    .tp-footer {
        margin-top: 4px;
        padding-top: 12px;
        border-top: 1px solid var(--color-divider, var(--color-border));
        display: flex;
        align-items: center;
        gap: 12px;
        font-size: 11.5px;
        color: var(--color-muted);
    }

    /* ─── Compact at narrow widths ───────────────────────────── */
    @media (max-width: 720px) {
        .tool-page {
            padding: 20px 16px 24px;
        }
        .tp-title {
            font-size: 20px;
        }
    }
</style>
