/*
  calcUnits — PURE unit system for the Calculator's Soulver-class engine.

  Linear units convert through a base unit via `toBase`; temperature is
  offset-based and handled separately. Absorbed from the former standalone
  Unit Converter tool. No DOM / Svelte deps → unit-testable in isolation.
*/

export type Dimension =
    | 'length'
    | 'mass'
    | 'temperature'
    | 'area'
    | 'volume'
    | 'time'
    | 'data'
    | 'speed';

export interface UnitDef {
    id: string; // canonical id, e.g. 'km'
    dim: Dimension;
    toBase: number; // value * toBase = value in the dimension's base unit
    display: string; // how to render it, e.g. 'km'
    aliases: string[]; // lowercase tokens that resolve to this unit
}

// Base units: length=m, mass=kg, area=m², volume=L, time=s, data=B, speed=m/s.
export const UNITS: UnitDef[] = [
    // length
    { id: 'm', dim: 'length', toBase: 1, display: 'm', aliases: ['m', 'meter', 'meters', 'metre', 'metres'] },
    { id: 'km', dim: 'length', toBase: 1000, display: 'km', aliases: ['km', 'kilometer', 'kilometers', 'kilometre', 'kilometres'] },
    { id: 'cm', dim: 'length', toBase: 0.01, display: 'cm', aliases: ['cm', 'centimeter', 'centimeters', 'centimetre', 'centimetres'] },
    { id: 'mm', dim: 'length', toBase: 0.001, display: 'mm', aliases: ['mm', 'millimeter', 'millimeters', 'millimetre', 'millimetres'] },
    { id: 'in', dim: 'length', toBase: 0.0254, display: 'in', aliases: ['in', 'inch', 'inches'] },
    { id: 'ft', dim: 'length', toBase: 0.3048, display: 'ft', aliases: ['ft', 'foot', 'feet'] },
    { id: 'yd', dim: 'length', toBase: 0.9144, display: 'yd', aliases: ['yd', 'yard', 'yards'] },
    { id: 'mi', dim: 'length', toBase: 1609.344, display: 'mi', aliases: ['mi', 'mile', 'miles'] },
    // mass
    { id: 'kg', dim: 'mass', toBase: 1, display: 'kg', aliases: ['kg', 'kilogram', 'kilograms', 'kilo', 'kilos'] },
    { id: 'g', dim: 'mass', toBase: 0.001, display: 'g', aliases: ['g', 'gram', 'grams'] },
    { id: 'mg', dim: 'mass', toBase: 1e-6, display: 'mg', aliases: ['mg', 'milligram', 'milligrams'] },
    { id: 't', dim: 'mass', toBase: 1000, display: 't', aliases: ['t', 'tonne', 'tonnes', 'ton', 'tons'] },
    { id: 'lb', dim: 'mass', toBase: 0.45359237, display: 'lb', aliases: ['lb', 'lbs', 'pound', 'pounds'] },
    { id: 'oz', dim: 'mass', toBase: 0.028349523125, display: 'oz', aliases: ['oz', 'ounce', 'ounces'] },
    { id: 'st', dim: 'mass', toBase: 6.35029318, display: 'st', aliases: ['st', 'stone', 'stones'] },
    // time
    { id: 'ms', dim: 'time', toBase: 0.001, display: 'ms', aliases: ['ms', 'millisecond', 'milliseconds'] },
    { id: 's', dim: 'time', toBase: 1, display: 's', aliases: ['s', 'sec', 'secs', 'second', 'seconds'] },
    { id: 'min', dim: 'time', toBase: 60, display: 'min', aliases: ['min', 'mins', 'minute', 'minutes'] },
    { id: 'h', dim: 'time', toBase: 3600, display: 'h', aliases: ['h', 'hr', 'hrs', 'hour', 'hours'] },
    { id: 'day', dim: 'time', toBase: 86400, display: 'day', aliases: ['day', 'days'] },
    { id: 'week', dim: 'time', toBase: 604800, display: 'week', aliases: ['week', 'weeks', 'wk', 'wks'] },
    // data (SI / decimal)
    { id: 'B', dim: 'data', toBase: 1, display: 'B', aliases: ['b', 'byte', 'bytes'] },
    { id: 'KB', dim: 'data', toBase: 1e3, display: 'KB', aliases: ['kb', 'kilobyte', 'kilobytes'] },
    { id: 'MB', dim: 'data', toBase: 1e6, display: 'MB', aliases: ['mb', 'megabyte', 'megabytes'] },
    { id: 'GB', dim: 'data', toBase: 1e9, display: 'GB', aliases: ['gb', 'gigabyte', 'gigabytes'] },
    { id: 'TB', dim: 'data', toBase: 1e12, display: 'TB', aliases: ['tb', 'terabyte', 'terabytes'] },
    // speed
    { id: 'm/s', dim: 'speed', toBase: 1, display: 'm/s', aliases: ['m/s', 'mps'] },
    { id: 'km/h', dim: 'speed', toBase: 1 / 3.6, display: 'km/h', aliases: ['km/h', 'kmh', 'kph'] },
    { id: 'mph', dim: 'speed', toBase: 0.44704, display: 'mph', aliases: ['mph'] },
    // area
    { id: 'm2', dim: 'area', toBase: 1, display: 'm²', aliases: ['m2', 'sqm'] },
    { id: 'km2', dim: 'area', toBase: 1e6, display: 'km²', aliases: ['km2', 'sqkm'] },
    { id: 'ft2', dim: 'area', toBase: 0.09290304, display: 'ft²', aliases: ['ft2', 'sqft'] },
    { id: 'acre', dim: 'area', toBase: 4046.8564224, display: 'acre', aliases: ['acre', 'acres'] },
    { id: 'ha', dim: 'area', toBase: 10000, display: 'ha', aliases: ['ha', 'hectare', 'hectares'] },
    // volume
    { id: 'L', dim: 'volume', toBase: 1, display: 'L', aliases: ['l', 'liter', 'liters', 'litre', 'litres'] },
    { id: 'mL', dim: 'volume', toBase: 0.001, display: 'mL', aliases: ['ml', 'milliliter', 'milliliters'] },
    { id: 'gal', dim: 'volume', toBase: 3.785411784, display: 'gal', aliases: ['gal', 'gallon', 'gallons'] },
];

/** Temperature is offset-based, so it lives outside the linear table. */
const TEMP_ALIASES: Record<string, 'c' | 'f' | 'k'> = {
    c: 'c', '°c': 'c', celsius: 'c',
    f: 'f', '°f': 'f', fahrenheit: 'f',
    k: 'k', kelvin: 'k',
};

const aliasMap = new Map<string, UnitDef>();
for (const u of UNITS) for (const a of u.aliases) aliasMap.set(a, u);

export type ResolvedUnit = { kind: 'linear'; def: UnitDef } | { kind: 'temp'; t: 'c' | 'f' | 'k' };

/** Resolve a token (case-insensitive, plural-tolerant) to a unit, or null. */
export function resolveUnit(token: string): ResolvedUnit | null {
    const k = token.trim().toLowerCase();
    if (!k) return null;
    if (k in TEMP_ALIASES) return { kind: 'temp', t: TEMP_ALIASES[k] };
    const def = aliasMap.get(k);
    return def ? { kind: 'linear', def } : null;
}

export function dimensionOf(u: ResolvedUnit): Dimension {
    return u.kind === 'temp' ? 'temperature' : u.def.dim;
}

export function sameDimension(a: ResolvedUnit, b: ResolvedUnit): boolean {
    return dimensionOf(a) === dimensionOf(b);
}

/** Convert a value from one unit to another in the SAME dimension. */
export function convert(value: number, from: ResolvedUnit, to: ResolvedUnit): number {
    if (!sameDimension(from, to)) throw new Error('Incompatible units');
    if (from.kind === 'temp' && to.kind === 'temp') {
        const k = from.t === 'c' ? value + 273.15 : from.t === 'f' ? ((value - 32) * 5) / 9 + 273.15 : value;
        return to.t === 'c' ? k - 273.15 : to.t === 'f' ? ((k - 273.15) * 9) / 5 + 32 : k;
    }
    if (from.kind === 'linear' && to.kind === 'linear') {
        return (value * from.def.toBase) / to.def.toBase;
    }
    throw new Error('Incompatible units');
}

export function unitDisplay(u: ResolvedUnit): string {
    return u.kind === 'temp' ? (u.t === 'c' ? '°C' : u.t === 'f' ? '°F' : 'K') : u.def.display;
}
