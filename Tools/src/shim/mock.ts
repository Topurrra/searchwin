// What the pages get when opened outside Search, for working on them in an
// ordinary browser (`pnpm dev`). Enough to render; nothing real happens.

const settings: Record<string, unknown> = {};

export async function mockCall(cmd: string, args: Record<string, unknown>): Promise<unknown> {
    switch (cmd) {
        case 'host:path':
            return args.which === 'temp' ? 'C:\\Temp' : 'C:\\Users\\you';
        case 'host:app.version':
            return 'dev';
        case 'host:dialog.confirm':
            return window.confirm(String(args.message ?? ''));
        case 'host:dialog.open':
        case 'host:dialog.save':
            return null;
        case 'load_app_settings':
            return settings;
        case 'save_app_settings':
            Object.assign(settings, args.settings ?? {});
            return null;
        case 'load_enabled_tool_packs':
            return null;
        case 'get_backend_init_issues':
            return [];
        case 'take_db_corruption_notice':
            return false;
        case 'encode_decode':
            if (args.algorithm === 'base64' && args.mode === 'encode') return btoa(String(args.input ?? ''));
            break;
    }
    if (cmd.startsWith('host:')) return null;
    throw `no engine here (dev mock): ${cmd}`;
}
