import { fireEvent, render } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import SegmentedControl from './SegmentedControl.svelte';

const options = [
  { value: 'system', label: 'System' },
  { value: 'light', label: 'Light' },
  { value: 'dark', label: 'Dark' },
];

describe('SegmentedControl', () => {
  it('checks exactly one option and reports clicks on another', async () => {
    const onChange = vi.fn();
    const { getAllByRole, getByRole } = render(SegmentedControl, {
      props: { options, value: 'light', onChange, ariaLabel: 'Theme' },
    });

    expect(getByRole('radiogroup', { name: 'Theme' })).toBeTruthy();
    const checked = getAllByRole('radio').filter((radio) => radio.ariaChecked === 'true');
    expect(checked.map((radio) => radio.textContent?.trim())).toEqual(['Light']);

    await fireEvent.click(getByRole('radio', { name: 'Dark' }));
    expect(onChange).toHaveBeenCalledWith('dark');
  });

  it('moves the choice with arrow keys, wrapping at the ends', async () => {
    const onChange = vi.fn();
    const { getByRole } = render(SegmentedControl, {
      props: { options, value: 'dark', onChange, ariaLabel: 'Theme' },
    });

    const group = getByRole('radiogroup');
    await fireEvent.keyDown(group, { key: 'ArrowRight' });
    expect(onChange).toHaveBeenLastCalledWith('system');
    await fireEvent.keyDown(group, { key: 'ArrowLeft' });
    expect(onChange).toHaveBeenLastCalledWith('light');
    await fireEvent.keyDown(group, { key: 'Home' });
    expect(onChange).toHaveBeenLastCalledWith('system');
  });

  it('blocks pointer and keyboard changes while disabled and re-enables normally', async () => {
    const onChange = vi.fn();
    const { getByRole, rerender } = render(SegmentedControl, {
      props: { options, value: 'light', onChange, ariaLabel: 'Theme', disabled: true },
    });
    const group = getByRole('radiogroup');
    expect(getByRole('radio', { name: 'Dark' })).toHaveProperty('disabled', true);
    await fireEvent.click(getByRole('radio', { name: 'Dark' }));
    await fireEvent.keyDown(group, { key: 'ArrowRight' });
    expect(onChange).not.toHaveBeenCalled();
    await rerender({ disabled: false });
    await fireEvent.keyDown(group, { key: 'ArrowRight' });
    expect(onChange).toHaveBeenCalledWith('dark');
  });

  it('keeps only the checked option in the tab order', () => {
    const { getAllByRole } = render(SegmentedControl, {
      props: { options, value: 'light', onChange: vi.fn(), ariaLabel: 'Theme' },
    });

    expect(getAllByRole('radio').map((radio) => radio.tabIndex)).toEqual([-1, 0, -1]);
  });
});
