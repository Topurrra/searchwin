<script lang="ts">
    import { Copy, RefreshCw, Download, Plus, Trash2 } from '@lucide/svelte';
    import { save } from '@tauri-apps/plugin-dialog';
    import { writeTextFile } from '@tauri-apps/plugin-fs';
    import { toast } from '$lib/stores/toasts';
    import { errorToast } from '$lib/stores/errorToast';
    import { Button, ToolPanel } from '$lib/ui';

    type DataType =
        | 'name' | 'first_name' | 'last_name' | 'email' | 'username'
        | 'phone' | 'address' | 'city' | 'country'
        | 'company' | 'job_title'
        | 'date' | 'iso_datetime' | 'unix_timestamp'
        | 'uuid' | 'integer' | 'float' | 'boolean'
        | 'lorem_word' | 'lorem_sentence' | 'lorem_paragraph'
        | 'iban' | 'credit_card' | 'ipv4' | 'mac_address'
        | 'url' | 'hex_color';

    type FieldDef = { name: string; type: DataType };

    type OutputFormat = 'json' | 'csv' | 'sql' | 'lines';

    let count = $state(20);
    let format = $state<OutputFormat>('json');
    let tableName = $state('users');
    let fields = $state<FieldDef[]>([
        { name: 'id', type: 'uuid' },
        { name: 'name', type: 'name' },
        { name: 'email', type: 'email' },
        { name: 'created_at', type: 'iso_datetime' },
    ]);

    // ----- Data pools -----
    const firstNames = ['James','Mary','John','Patricia','Robert','Jennifer','Michael','Linda','William','Elizabeth','David','Barbara','Richard','Susan','Joseph','Jessica','Thomas','Karen','Charles','Sarah','Christopher','Lisa','Daniel','Nancy','Matthew','Sandra','Anthony','Donna','Mark','Carol','Donald','Ruth','Steven','Sharon','Paul','Michelle','Andrew','Laura','Joshua','Sarah','Kenneth','Kimberly','Kevin','Deborah','Brian','Dorothy','George','Amy','Edward','Angela'];
    const lastNames = ['Smith','Johnson','Williams','Brown','Jones','Garcia','Miller','Davis','Rodriguez','Martinez','Hernandez','Lopez','Gonzalez','Wilson','Anderson','Thomas','Taylor','Moore','Jackson','Martin','Lee','Perez','Thompson','White','Harris','Sanchez','Clark','Ramirez','Lewis','Robinson','Walker','Young','Allen','King','Wright','Scott','Torres','Nguyen','Hill','Flores'];
    const cities = ['New York','Los Angeles','Chicago','Houston','Phoenix','Philadelphia','San Antonio','San Diego','Dallas','San Jose','Austin','London','Berlin','Paris','Madrid','Rome','Amsterdam','Stockholm','Tokyo','Sydney','Toronto','Barcelona','Munich','Vienna','Prague','Tbilisi','Warsaw','Dublin','Brussels','Lisbon'];
    const countries = ['USA','Canada','UK','Germany','France','Spain','Italy','Netherlands','Sweden','Japan','Australia','Brazil','Mexico','Argentina','Georgia','Poland','Czech Republic','Austria','Belgium','Portugal'];
    const streets = ['Main St','Oak Ave','Maple Rd','Cedar Ln','Pine St','Elm St','Washington Ave','Park Ave','First St','Second St','Lake Dr','River Rd','Hill St'];
    const companies = ['Acme Corp','Globex','Initech','Umbrella','Stark Industries','Wayne Enterprises','Wonka Industries','Cyberdyne','Soylent','Pied Piper','Hooli','Vandelay Industries','Massive Dynamic','Tyrell Corp','Oscorp','InGen'];
    const jobTitles = ['Software Engineer','Product Manager','Designer','Data Analyst','DevOps Engineer','CTO','CEO','Marketing Lead','Sales Director','Customer Success','QA Engineer','Tech Lead','Solutions Architect','Account Executive','Engineering Manager'];
    const tlds = ['com','io','net','co','dev','app','xyz'];
    const loremWords = ['lorem','ipsum','dolor','sit','amet','consectetur','adipiscing','elit','sed','do','eiusmod','tempor','incididunt','ut','labore','et','dolore','magna','aliqua','enim','ad','minim','veniam','quis','nostrud','exercitation','ullamco','laboris','nisi','aliquip','ex','ea','commodo','consequat','duis','aute','irure','reprehenderit','in','voluptate','velit','esse','cillum','fugiat','nulla','pariatur'];

    // ----- Generators -----
    function pick<T>(arr: T[]): T { return arr[Math.floor(Math.random() * arr.length)]; }
    function intBetween(min: number, max: number): number { return Math.floor(Math.random() * (max - min + 1)) + min; }
    function pad(n: number, len = 2) { return n.toString().padStart(len, '0'); }

    function genUuid(): string {
        return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, c => {
            const r = Math.random() * 16 | 0;
            const v = c === 'x' ? r : (r & 0x3 | 0x8);
            return v.toString(16);
        });
    }

    function genFirst() { return pick(firstNames); }
    function genLast() { return pick(lastNames); }
    function genName() { return `${genFirst()} ${genLast()}`; }
    function genUsername() { return `${genFirst().toLowerCase()}${intBetween(10, 999)}`; }
    function genEmail() { return `${genUsername()}@${pick(['example','test','mail','keepitlocal','sample'])}.${pick(tlds)}`; }
    function genPhone() {
        const cc = pick(['+1','+44','+49','+33','+995','+81','+61']);
        return `${cc} ${intBetween(100, 999)} ${intBetween(100, 999)} ${intBetween(1000, 9999)}`;
    }
    function genAddress() {
        return `${intBetween(1, 9999)} ${pick(streets)}, ${pick(cities)}`;
    }
    function genCompany() { return pick(companies); }
    function genJobTitle() { return pick(jobTitles); }

    function genIsoDateTime(): string {
        const now = new Date();
        const past = new Date(now.getTime() - intBetween(0, 365 * 24 * 3600 * 1000));
        return past.toISOString();
    }
    function genDate(): string {
        return genIsoDateTime().slice(0, 10);
    }
    function genUnixTimestamp(): number {
        return Math.floor(Date.now() / 1000) - intBetween(0, 365 * 24 * 3600);
    }

    function genIban(): string {
        const country = pick(['DE','GB','FR','ES','IT','NL']);
        const check = pad(intBetween(10, 99));
        let body = '';
        for (let i = 0; i < 18; i++) body += intBetween(0, 9);
        return `${country}${check}${body}`;
    }
    function genCreditCard(): string {
        const groups = [];
        for (let i = 0; i < 4; i++) groups.push(intBetween(1000, 9999));
        return groups.join(' ');
    }
    function genIpv4(): string {
        return `${intBetween(1, 255)}.${intBetween(0, 255)}.${intBetween(0, 255)}.${intBetween(1, 254)}`;
    }
    function genMac(): string {
        const parts = [];
        for (let i = 0; i < 6; i++) parts.push(pad(Number(intBetween(0, 255).toString(16)), 2));
        return parts.join(':');
    }
    function genUrl(): string {
        const protocol = Math.random() > 0.2 ? 'https' : 'http';
        const sub = pick(['www','app','api','blog','docs','']);
        const host = pick(['example','demo','test','sample','keepitlocal']);
        const tld = pick(tlds);
        const subPart = sub ? `${sub}.` : '';
        const path = Math.random() > 0.5 ? `/${pick(['users','products','orders','posts','docs'])}/${intBetween(1, 999)}` : '';
        return `${protocol}://${subPart}${host}.${tld}${path}`;
    }
    function genHexColor(): string {
        return '#' + Math.floor(Math.random() * 0xFFFFFF).toString(16).padStart(6, '0');
    }

    function genLoremWord() { return pick(loremWords); }
    function genLoremSentence(): string {
        const len = intBetween(5, 15);
        const words = [];
        for (let i = 0; i < len; i++) words.push(pick(loremWords));
        return words.join(' ').replace(/^./, c => c.toUpperCase()) + '.';
    }
    function genLoremParagraph(): string {
        const sentences = intBetween(3, 6);
        const out = [];
        for (let i = 0; i < sentences; i++) out.push(genLoremSentence());
        return out.join(' ');
    }

    function generateValue(type: DataType): any {
        switch (type) {
            case 'name': return genName();
            case 'first_name': return genFirst();
            case 'last_name': return genLast();
            case 'email': return genEmail();
            case 'username': return genUsername();
            case 'phone': return genPhone();
            case 'address': return genAddress();
            case 'city': return pick(cities);
            case 'country': return pick(countries);
            case 'company': return genCompany();
            case 'job_title': return genJobTitle();
            case 'date': return genDate();
            case 'iso_datetime': return genIsoDateTime();
            case 'unix_timestamp': return genUnixTimestamp();
            case 'uuid': return genUuid();
            case 'integer': return intBetween(1, 1000);
            case 'float': return Math.round(Math.random() * 10000) / 100;
            case 'boolean': return Math.random() > 0.5;
            case 'lorem_word': return genLoremWord();
            case 'lorem_sentence': return genLoremSentence();
            case 'lorem_paragraph': return genLoremParagraph();
            case 'iban': return genIban();
            case 'credit_card': return genCreditCard();
            case 'ipv4': return genIpv4();
            case 'mac_address': return genMac();
            case 'url': return genUrl();
            case 'hex_color': return genHexColor();
        }
    }

    // ----- Output formatting -----
    let output = $state('');

    function generate() {
        const rows: Record<string, any>[] = [];
        for (let i = 0; i < count; i++) {
            const row: Record<string, any> = {};
            for (const f of fields) {
                if (!f.name) continue;
                row[f.name] = generateValue(f.type);
            }
            rows.push(row);
        }

        output = formatRows(rows, format);
    }

    function formatRows(rows: Record<string, any>[], fmt: OutputFormat): string {
        if (rows.length === 0) return '';
        switch (fmt) {
            case 'json':
                return JSON.stringify(rows, null, 2);
            case 'csv': {
                const cols = Object.keys(rows[0]);
                const escape = (v: any) => {
                    const s = String(v);
                    return /[",\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
                };
                return [cols.join(','), ...rows.map(r => cols.map(c => escape(r[c])).join(','))].join('\n');
            }
            case 'sql': {
                const cols = Object.keys(rows[0]);
                const tn = tableName.trim() || 'data';
                const lines = rows.map(r => {
                    const vals = cols.map(c => {
                        const v = r[c];
                        if (typeof v === 'number') return String(v);
                        if (typeof v === 'boolean') return v ? 'TRUE' : 'FALSE';
                        return `'${String(v).replace(/'/g, "''")}'`;
                    }).join(', ');
                    return `INSERT INTO ${tn} (${cols.join(', ')}) VALUES (${vals});`;
                });
                return lines.join('\n');
            }
            case 'lines':
                return rows.map(r => Object.values(r).join('\t')).join('\n');
        }
    }

    // Auto-generate on changes (debounced for slider)
    let timeout: ReturnType<typeof setTimeout>;
    $effect(() => {
        fields;
        count;
        format;
        tableName;
        clearTimeout(timeout);
        timeout = setTimeout(generate, 100);
    });

    // ----- Field manipulation -----
    function addField() {
        fields = [...fields, { name: `field_${fields.length + 1}`, type: 'lorem_word' }];
    }
    function removeField(idx: number) {
        fields = fields.filter((_, i) => i !== idx);
    }
    function updateField(idx: number, patch: Partial<FieldDef>) {
        fields = fields.map((f, i) => i === idx ? { ...f, ...patch } : f);
    }

    async function copyOutput() {
        if (output) {
            await navigator.clipboard.writeText(output);
            toast('Copied output', 'success');
        }
    }

    async function saveOutput() {
        if (!output) return;
        const ext = format === 'json' ? 'json' : format === 'csv' ? 'csv' : format === 'sql' ? 'sql' : 'txt';
        const path = await save({
            defaultPath: `fake_data.${ext}`,
            filters: [{ name: format.toUpperCase(), extensions: [ext] }],
        });
        if (!path) return;
        try {
            await writeTextFile(path, output);
            toast('File saved', 'success');
        } catch (e) {
            errorToast("Couldn't save the generated data", e, {
                hint: 'Try a different folder — the destination may be read-only or out of space.',
                durationMs: 5500,
            });
        }
    }

    // Type categories for the dropdown
    const typeGroups: { label: string; types: { id: DataType; label: string }[] }[] = [
        {
            label: 'Identity',
            types: [
                { id: 'name', label: 'Full name' },
                { id: 'first_name', label: 'First name' },
                { id: 'last_name', label: 'Last name' },
                { id: 'username', label: 'Username' },
                { id: 'email', label: 'Email' },
                { id: 'phone', label: 'Phone' },
            ],
        },
        {
            label: 'Location',
            types: [
                { id: 'address', label: 'Address' },
                { id: 'city', label: 'City' },
                { id: 'country', label: 'Country' },
            ],
        },
        {
            label: 'Work',
            types: [
                { id: 'company', label: 'Company' },
                { id: 'job_title', label: 'Job title' },
            ],
        },
        {
            label: 'Numeric',
            types: [
                { id: 'integer', label: 'Integer (1-1000)' },
                { id: 'float', label: 'Float (0-100)' },
                { id: 'boolean', label: 'Boolean' },
            ],
        },
        {
            label: 'Date/Time',
            types: [
                { id: 'date', label: 'Date (YYYY-MM-DD)' },
                { id: 'iso_datetime', label: 'ISO 8601 datetime' },
                { id: 'unix_timestamp', label: 'Unix timestamp' },
            ],
        },
        {
            label: 'IDs / Codes',
            types: [
                { id: 'uuid', label: 'UUID v4' },
                { id: 'iban', label: 'IBAN (fake)' },
                { id: 'credit_card', label: 'Credit card (fake)' },
            ],
        },
        {
            label: 'Network',
            types: [
                { id: 'ipv4', label: 'IPv4' },
                { id: 'mac_address', label: 'MAC address' },
                { id: 'url', label: 'URL' },
            ],
        },
        {
            label: 'Visual',
            types: [
                { id: 'hex_color', label: 'Hex color' },
            ],
        },
        {
            label: 'Text',
            types: [
                { id: 'lorem_word', label: 'Lorem word' },
                { id: 'lorem_sentence', label: 'Lorem sentence' },
                { id: 'lorem_paragraph', label: 'Lorem paragraph' },
            ],
        },
    ];

    const formats: OutputFormat[] = ['json', 'csv', 'sql', 'lines'];
</script>

<div class="dt-panel">
    <div class="dt-head">
        <div class="dt-head-text">
            <h2 class="dt-title">Fake Data — generate realistic test fixtures</h2>
            <p class="dt-desc">Build sample datasets (names, emails, addresses, and more) and export as JSON, CSV, or SQL. Generated entirely on-device.</p>
        </div>
        <div class="dt-seg" role="group" aria-label="Output format">
            {#each formats as f}
                <button class="dt-seg-btn" class:is-active={format === f} onclick={() => (format = f)}>
                    {f.toUpperCase()}
                </button>
            {/each}
        </div>
    </div>

    <!-- Toolbar (actions) -->
    <div class="dt-toolbar">
        <Button variant="secondary" icon={Plus} onclick={addField}>Add field</Button>
        <div class="dt-toolbar-end">
            <Button variant="secondary" icon={RefreshCw} onclick={generate}>Regenerate</Button>
            <Button variant="secondary" icon={Copy} disabled={!output} onclick={copyOutput}>Copy</Button>
            <Button variant="primary" icon={Download} disabled={!output} onclick={saveOutput}>Save</Button>
        </div>
    </div>

    <div class="dt-grid">
        <!-- Output -->
        <div class="dt-col">
            <div class="dt-io">
                <div class="dt-io-head">
                    <span class="dt-section-label">Output</span>
                    <span class="dt-hint">{output.length} chars</span>
                </div>
                <textarea
                    class="dt-ta is-tall"
                    value={output}
                    readonly
                    spellcheck="false"
                ></textarea>
            </div>
        </div>

        <!-- Schema + options -->
        <ToolPanel padding="md">
            <div class="dt-col">
                <!-- Fields -->
                <div class="dt-io">
                    <div class="dt-io-head">
                        <span class="dt-section-label">Fields ({fields.length})</span>
                    </div>
                    <div class="dt-list">
                        {#each fields as f, i}
                            <div class="dt-row">
                                <input
                                    class="dt-input dt-mono"
                                    type="text"
                                    value={f.name}
                                    oninput={(e) => updateField(i, { name: e.currentTarget.value })}
                                    placeholder="field_name"
                                />
                                <select
                                    class="dt-select"
                                    value={f.type}
                                    onchange={(e) => updateField(i, { type: e.currentTarget.value as DataType })}
                                    aria-label="Field type"
                                >
                                    {#each typeGroups as g}
                                        <optgroup label={g.label}>
                                            {#each g.types as t}
                                                <option value={t.id}>{t.label}</option>
                                            {/each}
                                        </optgroup>
                                    {/each}
                                </select>
                                <button class="dt-link-btn" onclick={() => removeField(i)} aria-label="Remove field">
                                    <Trash2 class="dt-ico" />
                                </button>
                            </div>
                        {/each}
                    </div>
                </div>

                <!-- Count -->
                <div class="dt-field">
                    <label for="fake-count" class="dt-label">Rows: {count}</label>
                    <input id="fake-count" class="fd-range" type="range" min="1" max="1000" bind:value={count} />
                    <div class="fd-range-scale">
                        <span>1</span><span>500</span><span>1000</span>
                    </div>
                </div>

                {#if format === 'sql'}
                    <label class="dt-field" for="tbl">
                        <span class="dt-label">Table name</span>
                        <input
                            id="tbl"
                            class="dt-input dt-mono"
                            type="text"
                            bind:value={tableName}
                        />
                    </label>
                {/if}
            </div>
        </ToolPanel>
    </div>
</div>

<style>
    /* Local-only: range accent + scale, and inline icon sizing. The
       dt-* classes stay global (owned by the parent shell) and are
       never redefined here. */
    .fd-range {
        width: 100%;
        accent-color: var(--color-accent);
    }
    .fd-range-scale {
        display: flex;
        justify-content: space-between;
        margin-top: 2px;
        font-size: 10px;
        color: var(--color-muted);
    }
    .dt-panel :global(.dt-ico) {
        width: 14px;
        height: 14px;
        flex: none;
    }
</style>
