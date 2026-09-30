// The ports the e2e servers listen on (ADAGE_E2E_PORT moves both), away from the default 8765.
const base = Number(process.env.ADAGE_E2E_PORT ?? 47391);
export const DEMO_PORT = base;
export const EMPTY_PORT = base + 1;
