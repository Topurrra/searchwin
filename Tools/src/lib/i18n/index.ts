/*
  i18n entry point — wraps svelte-i18n so the rest of the app uses one
  small, swappable surface instead of importing the library everywhere.

  Usage in components:
      import { _ } from 'svelte-i18n';
      <span>{$_('statusBar.trustPill')}</span>
      <span>{$_('statusBar.files', { values: { count: 1234 } })}</span>

  Usage from non-component code (toast strings, etc.):
      import { t } from '$lib/i18n';
      toast(t('voice.transcribing', { seconds: 4.2 }), 'info');

  ── Locale bundle layout ─────────────────────────────────────────────
  Each locale's strings are split across many small JSON files under
  `locales/<locale>/*.json`, one file per app area (common, shell,
  settings, per tool-pack, …). Vite's `import.meta.glob` discovers
  them automatically and they are deep-merged into one message tree
  per locale. Benefits:
    • Adding an area = drop a new JSON file; no edit here needed.
    • Parallel work on different areas never conflicts (separate files).
    • Per-area diffs stay small and reviewable.
  Files may freely share top-level namespaces (e.g. several tool-pack
  files all contributing under `tool.*`) — the deep merge combines them.

  Adding a new locale:
    1. Add the id to `Locale` in `$lib/stores/settings.ts` + `LOCALE_OPTIONS`.
    2. Create `src/lib/i18n/locales/<id>/` and mirror the `en/` files.
    3. Register its glob below. Missing keys fall back to English.
*/

import { addMessages, init, getLocaleFromNavigator, _, locale } from 'svelte-i18n';
import { get } from 'svelte/store';
import type { Locale } from '$lib/stores/settings';

export { _, locale };

/** Recursively merge plain-object message trees. svelte-i18n wants one
 *  object per locale, but our source is many per-area files — and two
 *  files may both contribute under the same top-level namespace (all
 *  the tool-pack files land under `tool.*`). A shallow `Object.assign`
 *  would let the last file clobber siblings; this deep-merges instead.
 *  Leaf values (strings) from a later file win on an exact key clash. */
function deepMerge(
    target: Record<string, unknown>,
    source: Record<string, unknown>,
): Record<string, unknown> {
    for (const [key, value] of Object.entries(source)) {
        const existing = target[key];
        if (
            value &&
            typeof value === 'object' &&
            !Array.isArray(value) &&
            existing &&
            typeof existing === 'object' &&
            !Array.isArray(existing)
        ) {
            target[key] = deepMerge(
                existing as Record<string, unknown>,
                value as Record<string, unknown>,
            );
        } else {
            target[key] = value;
        }
    }
    return target;
}

/** Glob-import every per-area JSON file for a locale and deep-merge them
 *  into a single message tree. `eager: true` inlines the JSON at build
 *  time — no async loading, no flash. */
function loadBundle(
    modules: Record<string, unknown>,
): Record<string, unknown> {
    const merged: Record<string, unknown> = {};
    for (const mod of Object.values(modules)) {
        const data = (mod as { default?: unknown }).default ?? mod;
        if (data && typeof data === 'object') {
            deepMerge(merged, data as Record<string, unknown>);
        }
    }
    return merged;
}

// Vite resolves these globs at build time. Each pattern must be a string
// literal — no variables — so the two locales are listed explicitly.
const enBundle = loadBundle(
    import.meta.glob('./locales/en/*.json', { eager: true }),
);

/** True after `setupI18n()` has finished registering bundles + setting
 *  the initial locale. Components that render before this completes
 *  get raw key strings, so we initialize as early as possible. */
let initialized = false;

/** One-shot bootstrap. Call from the root +layout.svelte's onMount, or
 *  before importing any component that reads `$_`. Idempotent — repeat
 *  calls are no-ops so it's safe to call from multiple windows. */
export function setupI18n(initialLocale?: Locale) {
    if (initialized) {
        if (initialLocale) locale.set(initialLocale);
        return;
    }
    // The deep-merged bundles are typed as Record<string, unknown> (the
    // merge logic can't statically know the leaf shapes); svelte-i18n
    // wants its own LocaleDictionary. The runtime shape is correct —
    // nested objects of strings — so cast to the parameter's type.
    type MessagesArg = Parameters<typeof addMessages>[1];
    addMessages('en', enBundle as MessagesArg);
    init({
        // Fallback when a key is missing from the active locale's bundle.
        // Picking 'en' here means a half-translated locale shows English
        // for the missing keys instead of bare key paths — much better UX
        // than "statusBar.listening" leaking through.
        fallbackLocale: 'en',
        // Initial locale: explicit > navigator preference > English.
        initialLocale: initialLocale ?? getLocaleFromNavigator() ?? 'en',
    });
    initialized = true;
}

/** Imperative translator for code that runs outside a Svelte component
 *  (toast queues, store update callbacks, etc.). Internally pulls the
 *  same `_` store svelte-i18n exposes.
 *
 *  Accepts BOTH interpolation-argument shapes:
 *    • the `$_` component-store style — `t('key', { values: { count } })`
 *    • the flat style                — `t('key', { count })`
 *  Both are natural things to write, so `t()` normalizes rather than
 *  forcing callers to remember which. A 2nd arg is treated as the
 *  `{ values }` wrapper only when it has exactly a `values` property
 *  holding an object; otherwise the whole object is taken as the values
 *  record. Values are the primitives ICU MessageFormat supports
 *  (string/number/boolean/Date). */
type InterpolationValue = string | number | boolean | Date | null | undefined;
type ValuesRecord = Record<string, InterpolationValue>;
export function t(
    key: string,
    options?: { values?: ValuesRecord } | ValuesRecord,
): string {
    const translator = get(_);
    if (!options) return translator(key);
    const obj = options as Record<string, unknown>;
    const isWrapper =
        'values' in obj &&
        typeof obj.values === 'object' &&
        obj.values !== null &&
        !(obj.values instanceof Date);
    return translator(
        key,
        isWrapper
            ? (options as { values?: ValuesRecord })
            : { values: obj as ValuesRecord },
    );
}

/** Live setter — switches the active locale. Wired to the Settings
 *  page's language dropdown. The change is reactive, so every $_ binding
 *  in mounted components re-renders without a reload. */
export function setLocale(next: Locale) {
    locale.set(next);
}
