/*
  errorToast — one canonical helper for user-facing error toasts.

  Why this exists
  ───────────────
  The UX audit (`UxAudit.md`, TOP 5 #5) flagged the highest-leverage
  cross-cutting fix in the project: ~30 toast call-sites of the form
    `` `Couldn't <verb>: ${error}` ``
  which interpolate raw Rust `Result::Err` strings ("os error 3", "io
  error: The system cannot find the path specified") into the user's
  view. No situation framing, no "what to do next." Users see jargon
  and have no recovery path.

  The gold-standard counter-example already lives at
  `tools/Utils/VoiceToText.svelte:294-311` — it branches on known
  error codes into actionable plain-English toasts. This helper
  bakes that shape into one call so every sweep site uses the same
  pattern:

      situation — hint (— optional detail)

  Where:
    - `situation` is the failure in plain English ("Couldn't save the
      redacted image", "Couldn't read this image's metadata")
    - `hint` is what the user should DO ("Try a different folder.",
      "Check that you pasted the public key in PEM/JWK format.")
    - `detail` is an optional compact technical addendum, used
      sparingly — only when it genuinely helps (a path, a known
      code, the `os error N` part of a Windows error).

  The raw error itself is always written to the devtools console
  (`console.warn`) so developers can still diagnose, but never
  rendered into the toast unless the caller explicitly passes
  `detail`. Toast-level `recordLog` (in `stores/toasts.ts`) captures
  the user-facing message for the in-app diagnostic log automatically.
*/
import { toast } from './toasts';

export interface ErrorToastOptions {
    /** Plain-English next step for the user. Appended to `situation`
     *  with an em-dash. Almost always worth providing — the entire
     *  point of this helper is "every error message tells the user a
     *  recovery path." */
    hint?: string;
    /** Override the default toast duration (4500ms). Use longer for
     *  destructive failures (work potentially lost) where the user
     *  needs to read carefully. */
    durationMs?: number;
    /** Compact technical detail surfaced to the user in parentheses
     *  at the end of the toast. Use ONLY when it adds value — a
     *  file path, a known error code, an HTTP status. Never pass
     *  the raw `${error}` here; the helper itself decides where the
     *  raw error goes. */
    detail?: string;
}

/**
 * Show a user-facing error toast with the project's standard shape.
 *
 *   errorToast('Couldn't save the redacted image', err, {
 *       hint: 'Try a different folder — the destination may be read-only or full.',
 *   });
 *
 * @param situation Plain-English description of what failed. NEVER
 *   interpolate a raw `${error}` here — that's what defeats the
 *   purpose. Keep it stable so the diagnostic log can group repeats.
 * @param error    The original error/exception. Logged to console
 *   for devs; never shown to the user (use `opts.detail` for that).
 *   Pass `null` / `undefined` when there's no underlying error
 *   (e.g. a validation failure).
 * @param opts     `{ hint?, durationMs?, detail? }` — see above.
 */
export function errorToast(
    situation: string,
    error: unknown,
    opts: ErrorToastOptions = {},
): void {
    // Compose the user-visible message. Em-dash separator keeps the
    // shape consistent and avoids the `Couldn't X: <jargon>` pattern.
    const parts: string[] = [situation];
    if (opts.hint) parts.push(opts.hint);
    if (opts.detail) parts.push(`(${opts.detail})`);
    const message = parts.join(' — ');

    // Devtools breadcrumb — devs can still find the raw error during
    // diagnosis even though the user never sees it.
    if (error !== undefined && error !== null) {
        console.warn('[errorToast]', situation, error);
    }

    // The toast itself. `toast()` auto-records error-level entries
    // into the diagnostic log (`stores/toasts.ts:25-27`), so we
    // don't need to log separately — the user-facing message goes
    // in, devs grep the log for it.
    toast(message, 'error', opts.durationMs ?? 4500);
}
