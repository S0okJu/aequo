import { cleanup } from '@testing-library/svelte';
import { afterEach, beforeEach } from 'vitest';

import { setLocalePreference } from '$lib/i18n/index.svelte';

// Component tests query by English text; pin the UI language so they don't
// depend on the machine's locale.
beforeEach(() => setLocalePreference('en'));
afterEach(() => cleanup());
