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

// jsdom doesn't implement <dialog>'s showModal/close at all (not even as a
// no-op); the `open` attribute/property themselves already reflect
// correctly, so a minimal shim is enough for components that drive a
// native <dialog> (see AddFeedDialog.svelte, FeedSourceDialog.svelte).
if (typeof HTMLDialogElement !== 'undefined' && !HTMLDialogElement.prototype.showModal) {
  HTMLDialogElement.prototype.showModal = function (this: HTMLDialogElement) {
    this.setAttribute('open', '');
  };
  HTMLDialogElement.prototype.close = function (this: HTMLDialogElement) {
    this.removeAttribute('open');
    this.dispatchEvent(new Event('close'));
  };
}
