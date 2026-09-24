/*
  devkitPanels.ts — module-level state for the Developer Tools devkit panels.

  Why this exists
  ───────────────
  The app wraps the active tool in `{#key}` blocks (src/routes/+page.svelte and
  src/lib/CategoryWorkspace.svelte). Leaving the Developer Tools hub and
  returning UNMOUNTS + re-mounts each panel fresh, wiping any component-local
  `$state`. A user who typed a regex / SQL / diff / markdown and walked away
  would lose it.

  Lifting each panel's INPUT + computed RESULT into module-level writable stores
  keeps that work alive for the lifetime of the app session (same pattern as
  stores/duplicateFinder.ts). Panels read with `$store` and write with
  `bind:value={$store}` / `store.set(...)`.

  One section per panel below. Each section owns only the fields that need to
  survive navigation — ephemeral UI flags that are cheap to recompute (busy
  spinners, focus refs) stay component-local.
*/
import { writable } from 'svelte/store';


export type DevkitMode =
    | 'jwt'
    | 'regex'
    | 'sql'
    | 'diff'
    | 'idgen'
    | 'fakedata'
    | 'cron'
    | 'secretscan';

export const devkitMode = writable<DevkitMode>('jwt');
// ─── Regex panel ─────────────────────────────────────────────────────────────

export type RegexFlags = { g: boolean; i: boolean; m: boolean; s: boolean; u: boolean };

export type RegexMatch = {
    text: string;
    index: number;
    groups: string[];
    named: Record<string, string>;
};

export type RegexComputed = {
    /** Compile error message, or null when the pattern compiled cleanly. */
    error: string | null;
    matches: RegexMatch[];
    /** Highlight segments for the test string. */
    segments: { text: string; match: boolean; idx?: number }[];
    /** Find/replace preview output, or null when it couldn't be produced. */
    replaced: string | null;
    /** True when live matching was truncated because the test string exceeded
     *  the size cap — the UI shows an inline notice. */
    truncated: boolean;
};

export const regexTab = writable<'test' | 'build'>('build');
export const regexPattern = writable('\\b\\w+@\\w+\\.\\w+\\b');
export const regexTestString = writable(
    `Contact us at hello@example.com or support@keepitlocal.io.\nAlternative: admin@test.org`,
);
export const regexReplacement = writable('');
export const regexFlags = writable<RegexFlags>({ g: true, i: false, m: false, s: false, u: false });
export const regexExampleInput = writable('one@example.com\ntwo@foo.org\nthree@bar.net');
/** Debounced result of compiling + running the pattern. Written by the panel's
 *  debounced effect, read by the template. */
export const regexComputed = writable<RegexComputed>({
    error: null,
    matches: [],
    segments: [],
    replaced: null,
    truncated: false,
});

// ─── Diff panel ──────────────────────────────────────────────────────────────

export type DiffOp = 'eq' | 'ins' | 'del' | 'mod';
export type DiffRow = {
    op: DiffOp;
    leftLine?: number;
    leftText?: string;
    rightLine?: number;
    rightText?: string;
};

export type DiffComputed = {
    rows: DiffRow[];
    /** True when the inputs exceeded the size cap so auto-diff was skipped and
     *  the user must press Compare. */
    tooLarge: boolean;
};

export const diffLeftLabel = writable('Original');
export const diffRightLabel = writable('Modified');
export const diffLeftText = writable(`function greet(name) {
  console.log("Hello, " + name);
  return name.toUpperCase();
}`);
export const diffRightText = writable(`function greet(name, greeting = "Hello") {
  console.log(greeting + ", " + name);
  return name.toLowerCase();
}`);
export const diffMode = writable<'side' | 'unified'>('side');
export const diffIgnoreWhitespace = writable(false);
export const diffIgnoreCase = writable(false);
export const diffPiiAware = writable(false);
export const diffRevealPii = writable(false);
/** Debounced diff result, written by the panel's debounced effect. */
export const diffComputed = writable<DiffComputed>({ rows: [], tooLarge: false });

// ─── SQL panel ───────────────────────────────────────────────────────────────

export type SqlLintStat = { label: string; value: string };
export type SqlLintFinding = { severity: string; message: string };

export const sqlInput = writable(
    `update users set status='inactive';\nselect id,name,email from users u join orders o on u.id=o.user_id where u.created_at>'2024-01-01' order by u.name limit 100;`,
);
export const sqlOutput = writable('');
export const sqlIndent = writable(2);
export const sqlUppercase = writable(true);
export const sqlLinesBetween = writable(1);
export const sqlPreset = writable('readable');
export const sqlError = writable<string | null>(null);
export const sqlLintError = writable<string | null>(null);
export const sqlLintStats = writable<SqlLintStat[]>([]);
export const sqlLintFindings = writable<SqlLintFinding[]>([]);
export const sqlLintCount = writable(0);
export const sqlProcessing = writable(false);
export const sqlLintRunning = writable(false);

// ─── Secret scan panel ───────────────────────────────────────────────────────

// `SensitiveFinding` is imported by the panel from $lib/types/sensitive; the
// store keeps it loosely typed to avoid a circular/heavy import here.
export const secretScanInput = writable(
    `API_KEY=abc12345deadbeef9876543210\nDB_CONN=postgres://user:pass@localhost:5432/main\ndev@company.com\ncard=4111 1111 1111 1111`,
);
export const secretScanPreset = writable<'security' | 'privacy' | 'release'>('security');
export const secretScanAutoRedact = writable(true);
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export const secretScanFindings = writable<any[]>([]);
export const secretScanProcessing = writable(false);
export const secretScanError = writable<string | null>(null);

// ─── Markdown panel ──────────────────────────────────────────────────────────

export const markdownTab = writable<'md-to-html' | 'html-to-md'>('md-to-html');
export const markdownInput = writable(`# KeepItLocal

**Privacy-first** desktop toolkit. All tools run *locally* — no uploads, no telemetry.

## Tools

- Hash Check
- Encoders (Base64, Hex, URL...)
- QR Code generator
- Format Converter

## Code example

\`\`\`rust
fn main() {
    println!("Hello, KeepItLocal!");
}
\`\`\`

> "Stop uploading your files to sketchy websites."

[KeepItLocal](https://keepitlocal.app)`);
export const markdownHtmlInput = writable('');
export const markdownPreviewMode = writable<'split' | 'preview' | 'source'>('split');
export const markdownSourceDir = writable<string | null>(null);
