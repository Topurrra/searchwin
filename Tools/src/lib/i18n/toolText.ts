/*
  Tool catalog text helpers — resolve a tool's localized name/description
  from the `catalog.json` bundle, falling back to the English literal that
  ships in `appScreens.ts` when no translation key exists.

  Why a helper instead of inline `$_(...)`:
    - `appScreens.ts` deliberately keeps its English `name`/`description`
      literals (they feed voice-command + search matching). The UI, however,
      should render the localized catalog string.
    - svelte-i18n returns the key path itself when a key is missing. These
      helpers detect that ("v === k") and substitute the caller's English
      fallback so a half-translated locale never leaks "tools.foo.name".

  Use from non-reactive code (or where a one-shot read is fine). In Svelte
  markup that must re-render on locale change, prefer `{$_(\`tools.${id}.name\`)}`
  directly so the binding stays reactive.
*/
import { get } from 'svelte/store';
import { _ } from 'svelte-i18n';

export function toolDisplayName(id: string, fallback: string): string {
    const tr = get(_);
    const k = `tools.${id}.name`;
    const v = tr(k);
    return v === k ? fallback : v;
}

export function toolDisplayDesc(id: string, fallback: string): string {
    const tr = get(_);
    const k = `tools.${id}.desc`;
    const v = tr(k);
    return v === k ? fallback : v;
}
