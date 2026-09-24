/*
 * Shared TypeScript types for sensitive-content detection — Phase 6.5
 * (2026-05-27). Mirrors the Rust shapes in
 * `src-tauri/src/commands/sensitive_scan.rs`. Keep these in sync.
 *
 * Why a shared file: at least three frontend surfaces consume findings
 * (Secret Leak Scanner tool, Clipboard history previews, Privacy Audit's
 * Unencrypted Secrets section) plus the PrivacyBlur primitive that
 * renders them. One source of truth prevents drift.
 */

/**
 * Confidence tier for a sensitive-content match. The default blur policy
 * is tier-aware:
 *   • `high`   — prefixed/structured tokens + Luhn-validated cards.
 *                Blurred by default in clipboard + audit.
 *   • `medium` — validated noisy shapes (key=value, Bearer, SSN).
 *                Surfaced in the Secret Leak Scanner; not blurred elsewhere.
 *   • `low`    — generic PII (email, phone, IP). Off by default everywhere;
 *                only opted in via the SecretLeakScanner Privacy preset.
 */
export type SensitiveTier = 'high' | 'medium' | 'low';

/** One sensitive-content finding with its byte-range span in the
 *  scanned text. `text.slice(start, end)` recovers the matched
 *  substring on the JS side. */
export interface SensitiveFinding {
    kind: string;
    tier: SensitiveTier;
    start: number;
    end: number;
}

/** Cheap label lookup for the UI — what to show next to a finding
 *  ("AWS access key", "Credit card", etc.). Falls back to the raw
 *  kind id if we haven't curated a label for it yet. */
export function sensitiveKindLabel(kind: string): string {
    return KIND_LABELS[kind] ?? humanizeKind(kind);
}

const KIND_LABELS: Record<string, string> = {
    sensitive: 'Sensitive content',
    aws_access_key: 'AWS access key',
    github_token: 'GitHub token',
    slack_token: 'Slack token',
    slack_webhook: 'Slack webhook',
    stripe_secret_key: 'Stripe secret key',
    google_api_key: 'Google API key',
    twilio_account_sid: 'Twilio account SID',
    sendgrid_api_key: 'SendGrid API key',
    jwt_token: 'JWT token',
    private_key_pem: 'PEM private key',
    pgp_private_key: 'PGP private key',
    database_url_with_password: 'Database URL with password',
    ethereum_private_key: 'Ethereum private key',
    bitcoin_wif_private_key: 'Bitcoin private key (WIF)',
    anthropic_api_key: 'Anthropic API key',
    openai_api_key: 'OpenAI API key',
    npm_token: 'npm token',
    bitcoin_address: 'Bitcoin address',
    bitcoin_bech32: 'Bitcoin address',
    ethereum_address: 'Ethereum address',
    solana_address: 'Solana address',
    generic_credential_assignment: 'Credential assignment',
    bearer_token: 'Bearer token',
    us_ssn: 'US SSN',
    credit_card: 'Credit card',
    iban: 'IBAN',
    italian_cf: 'Italian fiscal code',
    india_aadhaar: 'Aadhaar number',
    us_itin: 'US ITIN',
    us_ein: 'US EIN',
    uk_nino: 'UK NINO',
    france_nir: 'France NIR',
    vin: 'VIN',
    spain_dni: 'Spain DNI/NIE',
    dutch_bsn: 'Dutch BSN',
    mac_address: 'MAC address',
    passport: 'Passport number',
    drivers_license: "Driver's license",
    bank_account: 'Bank account',
    medical_record_number: 'Medical record #',
    dob: 'Date of birth',
    routing_number: 'Routing number',
    georgian_id: 'Georgian ID',
    email: 'Email address',
    phone: 'Phone number',
    ipv4: 'IP address',
    date: 'Date',
};

function humanizeKind(kind: string): string {
    return kind.replace(/_/g, ' ').replace(/\b\w/g, (c) => c.toUpperCase());
}

/* ─── Kind → tier lookup ───────────────────────────────────────────────
 *
 * Mirror of the tier assignment in
 * `src-tauri/src/commands/sensitive_scan.rs`. Used by surfaces that
 * only know the kind LABELS (e.g. clipboard history persists
 * `sensitive_kinds: string[]`, no tiers) but need tier-aware behavior
 * like "only blur HIGH-tier preview text".
 *
 * Keep these sets in sync with the backend. The backend remains the
 * source of truth — if a kind is missing here, `kindToTier` returns
 * null and the consumer treats it as "don't apply tier-specific
 * behavior", which is the safe default.
 */

const HIGH_KINDS = new Set<string>([
    'aws_access_key',
    'github_token',
    'slack_token',
    'slack_webhook',
    'stripe_secret_key',
    'google_api_key',
    'twilio_account_sid',
    'sendgrid_api_key',
    'jwt_token',
    'private_key_pem',
    'pgp_private_key',
    'database_url_with_password',
    'ethereum_private_key',
    'bitcoin_wif_private_key',
    'anthropic_api_key',
    'openai_api_key',
    'npm_token',
    'bitcoin_address',
    'bitcoin_bech32',
    'ethereum_address',
    'solana_address',
    'credit_card',
]);

const MEDIUM_KINDS = new Set<string>([
    'generic_credential_assignment',
    'bearer_token',
    'us_ssn',
    'iban',
    'italian_cf',
    'uk_nino',
    'france_nir',
    'us_itin',
    'vin',
    'spain_dni',
    'passport',
    'drivers_license',
    'bank_account',
    'medical_record_number',
    'dob',
    'routing_number',
]);

const LOW_KINDS = new Set<string>([
    'email',
    'phone',
    'ipv4',
    'mac_address',
    'date',
    'georgian_id',
    'india_aadhaar',
    'us_ein',
    'dutch_bsn',
]);

/** Map a kind label to its tier. Returns null for unknown kinds and
 *  for the "sensitive" universal tag (which isn't a real tier — it's
 *  just a marker that *something* matched). */
export function kindToTier(kind: string): SensitiveTier | null {
    if (HIGH_KINDS.has(kind)) return 'high';
    if (MEDIUM_KINDS.has(kind)) return 'medium';
    if (LOW_KINDS.has(kind)) return 'low';
    return null;
}

/** True when at least one kind in `kinds` is HIGH-tier. Drives the
 *  clipboard preview blur (only HIGH-tier entries hide their
 *  contents). LOW-tier matches like email/phone don't deserve a
 *  shoulder-surf shield in the clipboard list. */
export function hasHighTierKind(kinds: string[] | null | undefined): boolean {
    if (!kinds) return false;
    return kinds.some((k) => HIGH_KINDS.has(k));
}
