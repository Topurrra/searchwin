/** @type {import('tailwindcss').Config} */

/*
 * Theme-aware Tailwind palette.
 *
 * Every status-y color family (emerald/rose/amber/blue/etc.) is redefined
 * here as `rgb(var(--tw-COLOR-SHADE) / <alpha-value>)` — which means every
 * existing `text-emerald-300`, `bg-rose-500/10`, etc. across the codebase
 * automatically becomes theme-aware. Each theme (`styles.css`) just supplies
 * its own RGB triplets for those CSS variables. No per-file refactor needed.
 *
 * The `<alpha-value>` placeholder is Tailwind 3's way of preserving opacity
 * modifiers — `bg-emerald-500/10` still gets `rgb(... / 0.1)` behavior,
 * just with the theme's emerald instead of Tailwind's default.
 *
 * Fallback RGB after the comma (e.g. `var(--tw-emerald-300, 110 231 183)`)
 * is the dark-theme default — used if a theme forgets to set the variable
 * so the UI stays stable rather than rendering invisible.
 *
 * Only the shades actually in use across the codebase are listed
 * (audited via `grep -E '(emerald|rose|...)-(50|100|...|900)'` against /src).
 */

function shade(name, fallback) {
    return `rgb(var(--tw-${name}, ${fallback}) / <alpha-value>)`;
}

export default {
  content: ["./src/**/*.{html,svelte,js,ts}"],
  theme: {
    extend: {
      colors: {
        // ─── Surface + base tokens ─────────────────────────────────
        bg: 'var(--color-bg)',
        panel: 'var(--color-panel)',
        'panel-2': 'var(--color-panel-2)',
        'panel-3': 'var(--color-panel-3)',
        border: 'var(--color-border)',
        'border-strong': 'var(--color-border-strong)',
        text: 'var(--color-text)',
        'text-secondary': 'var(--color-text-secondary)',
        muted: 'var(--color-muted)',

        // ─── Brand / accent ────────────────────────────────────────
        // `accent` uses the rgb()/alpha-value pattern so opacity modifiers
        // (`bg-accent/15`, `text-accent/75`, `border-accent/30`) compose
        // correctly. Solid `bg-accent` still works — Tailwind expands the
        // placeholder to `1` when no opacity modifier is present.
        // The other tokens stay as plain `var(...)` since they're never
        // opacity-modified and a pre-computed `accent-soft` is more
        // efficient than rebuilding the soft tint each time.
        accent: 'rgb(var(--tw-accent-rgb, 16 185 129) / <alpha-value>)',
        'accent-hover': 'var(--color-accent-hover)',
        'accent-soft': 'var(--color-accent-soft)',
        'accent-contrast': 'var(--color-accent-contrast)',

        // ─── Semantic status tokens (use these for new code) ──────
        // Each status has three forms:
        //   `<name>`         — solid (text/borders/icons)
        //   `<name>-soft`    — translucent fill (bg of alerts, badges)
        //   `<name>-strong`  — emphasized border / focused state
        success: 'var(--color-success)',
        'success-soft': 'var(--color-success-soft)',
        'success-strong': 'var(--color-success-strong)',
        warning: 'var(--color-warning)',
        'warning-soft': 'var(--color-warning-soft)',
        'warning-strong': 'var(--color-warning-strong)',
        error: 'var(--color-error)',
        'error-soft': 'var(--color-error-soft)',
        'error-strong': 'var(--color-error-strong)',
        info: 'var(--color-info)',
        'info-soft': 'var(--color-info-soft)',
        'info-strong': 'var(--color-info-strong)',

        // ─── Theme-aware Tailwind palette (legacy compatibility) ──
        // Every existing `text-emerald-300`, `bg-rose-500/10`, etc.
        // across the codebase passes through these variables, so
        // switching theme automatically retones every status-y badge,
        // toast, banner, etc. across every screen.
        emerald: {
            50:  shade('emerald-50',  '236 253 245'),
            100: shade('emerald-100', '209 250 229'),
            200: shade('emerald-200', '167 243 208'),
            300: shade('emerald-300', '110 231 183'),
            400: shade('emerald-400', '52 211 153'),
            500: shade('emerald-500', '16 185 129'),
            700: shade('emerald-700', '4 120 87'),
            900: shade('emerald-900', '6 78 59'),
        },
        green: {
            200: shade('green-200', '187 247 208'),
            300: shade('green-300', '134 239 172'),
            400: shade('green-400', '74 222 128'),
            500: shade('green-500', '34 197 94'),
        },
        lime: {
            400: shade('lime-400', '163 230 53'),
            500: shade('lime-500', '132 204 22'),
        },
        teal: {
            500: shade('teal-500', '20 184 166'),
        },
        red: {
            200: shade('red-200', '254 202 202'),
            300: shade('red-300', '252 165 165'),
            400: shade('red-400', '248 113 113'),
            500: shade('red-500', '239 68 68'),
            600: shade('red-600', '220 38 38'),
            800: shade('red-800', '153 27 27'),
            900: shade('red-900', '127 29 29'),
        },
        rose: {
            300: shade('rose-300', '253 164 175'),
            500: shade('rose-500', '244 63 94'),
        },
        pink: {
            400: shade('pink-400', '244 114 182'),
            500: shade('pink-500', '236 72 153'),
        },
        amber: {
            100: shade('amber-100', '254 243 199'),
            200: shade('amber-200', '253 230 138'),
            300: shade('amber-300', '252 211 77'),
            400: shade('amber-400', '251 191 36'),
            500: shade('amber-500', '245 158 11'),
        },
        yellow: {
            200: shade('yellow-200', '254 240 138'),
            300: shade('yellow-300', '253 224 71'),
            400: shade('yellow-400', '250 204 21'),
            900: shade('yellow-900', '113 63 18'),
        },
        orange: {
            300: shade('orange-300', '253 186 116'),
            400: shade('orange-400', '251 146 60'),
            500: shade('orange-500', '249 115 22'),
            900: shade('orange-900', '124 45 18'),
        },
        blue: {
            400: shade('blue-400', '96 165 250'),
            500: shade('blue-500', '59 130 246'),
            600: shade('blue-600', '37 99 235'),
            900: shade('blue-900', '30 58 138'),
        },
        sky: {
            400: shade('sky-400', '56 189 248'),
            500: shade('sky-500', '14 165 233'),
        },
        cyan: {
            300: shade('cyan-300', '103 232 249'),
            400: shade('cyan-400', '34 211 238'),
        },
        indigo: {
            300: shade('indigo-300', '165 180 252'),
        },
        violet: {
            400: shade('violet-400', '167 139 250'),
        },
        purple: {
            400: shade('purple-400', '192 132 252'),
            500: shade('purple-500', '168 85 247'),
        },
        fuchsia: {
            300: shade('fuchsia-300', '240 171 252'),
        },
      },
      fontFamily: {
        sans: ['Inter', 'system-ui', 'sans-serif'],
        mono: ['JetBrains Mono', 'Consolas', 'monospace'],
      },
    },
  },
  plugins: [],
};
