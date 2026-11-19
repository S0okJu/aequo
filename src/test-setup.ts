import { cleanup } from '@testing-library/svelte';
import { afterEach, beforeEach } from 'vitest';

import { setLocalePreference } from '$lib/i18n/index.svelte';

// Component tests query by English text; pin the UI language so they don't
// depend on the machine's locale.
beforeEach(() => setLocalePreference('en'));
afterEach(() => cleanup());

// jsdom has no Web Animations API; Svelte's `in:`/`transition:` directives
// call `element.animate`. A finished no-op animation is enough for tests.
if (typeof Element !== 'undefined' && !Element.prototype.animate) {
  Element.prototype.animate = function () {
    const animation = {
      onfinish: null as (() => void) | null,
      cancel() {},
      finish() {},
      currentTime: 0,
    };
    queueMicrotask(() => animation.onfinish?.());
    return animation as unknown as Animation;
  };
}

if (typeof Element !== 'undefined' && !Element.prototype.getAnimations) {
  Element.prototype.getAnimations = () => [];
}
