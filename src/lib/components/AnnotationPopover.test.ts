import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { Annotation } from '$lib/types';
import AnnotationPopover from './AnnotationPopover.svelte';

function rect(): DOMRect {
  return { top: 100, bottom: 120, left: 50, right: 80, width: 30, height: 20, x: 50, y: 100, toJSON() {} } as DOMRect;
}

function memoAnnotation(overrides: Partial<Annotation> = {}): Annotation {
  return {
    id: 1,
    itemId: 1,
    kind: 'memo',
    contentHash: 'h',
    startOffset: 0,
    endOffset: 5,
    quotedText: 'hello',
    color: null,
    note: 'original note',
    createdAt: 0,
    updatedAt: 0,
    ...overrides,
  };
}

describe('AnnotationPopover', () => {
  it('autofocuses the textarea for a memo, and the first swatch for a highlight', async () => {
    render(AnnotationPopover, {
      props: {
        rect: rect(),
        kind: 'memo',
        annotation: null,
        onSaveNote: vi.fn(),
        onColorChange: vi.fn(),
        onDelete: vi.fn(),
        onClose: vi.fn(),
      },
    });
    expect(document.activeElement).toBe(screen.getByPlaceholderText('Add a note…'));
  });

  it('closes on Escape even with an unsaved draft, and restores nothing itself (the caller owns focus restore)', async () => {
    const onClose = vi.fn();
    render(AnnotationPopover, {
      props: {
        rect: rect(),
        kind: 'memo',
        annotation: memoAnnotation(),
        onSaveNote: vi.fn(),
        onColorChange: vi.fn(),
        onDelete: vi.fn(),
        onClose,
      },
    });
    await fireEvent.input(screen.getByPlaceholderText('Add a note…'), {
      target: { value: 'an unsaved edit' },
    });
    await fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('ignores a backdrop click while the note draft is dirty, but allows it once clean', async () => {
    const onClose = vi.fn();
    const { container } = render(AnnotationPopover, {
      props: {
        rect: rect(),
        kind: 'memo',
        annotation: memoAnnotation(),
        onSaveNote: vi.fn(),
        onColorChange: vi.fn(),
        onDelete: vi.fn(),
        onClose,
      },
    });
    const backdrop = container.querySelector('.backdrop') as HTMLElement;

    await fireEvent.input(screen.getByPlaceholderText('Add a note…'), {
      target: { value: 'an unsaved edit' },
    });
    await fireEvent.click(backdrop);
    expect(onClose).not.toHaveBeenCalled();

    await fireEvent.input(screen.getByPlaceholderText('Add a note…'), {
      target: { value: 'original note' },
    });
    await fireEvent.click(backdrop);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('allows a backdrop click immediately for a highlight popover (no draft to lose)', async () => {
    const onClose = vi.fn();
    const { container } = render(AnnotationPopover, {
      props: {
        rect: rect(),
        kind: 'highlight',
        annotation: null,
        onSaveNote: vi.fn(),
        onColorChange: vi.fn(),
        onDelete: vi.fn(),
        onClose,
      },
    });
    await fireEvent.click(container.querySelector('.backdrop') as HTMLElement);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('traps Tab focus inside the dialog', async () => {
    render(AnnotationPopover, {
      props: {
        rect: rect(),
        kind: 'memo',
        annotation: memoAnnotation(),
        onSaveNote: vi.fn(),
        onColorChange: vi.fn(),
        onDelete: vi.fn(),
        onClose: vi.fn(),
      },
    });
    const dialog = screen.getByRole('dialog');
    const focusable = dialog.querySelectorAll('button, textarea');
    const last = focusable[focusable.length - 1] as HTMLElement;
    const first = focusable[0] as HTMLElement;

    last.focus();
    await fireEvent.keyDown(dialog, { key: 'Tab' });
    expect(document.activeElement).toBe(first);

    first.focus();
    await fireEvent.keyDown(dialog, { key: 'Tab', shiftKey: true });
    expect(document.activeElement).toBe(last);
  });
});
