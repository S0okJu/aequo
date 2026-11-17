<script lang="ts">
  import { createFeedFolder, deleteFeedFolder, renameFeedFolder } from '$lib/api';
  import Icon from '$lib/icons/Icon.svelte';
  import type { FeedFolder, FeedSummary } from '$lib/types';

  interface Props {
    feeds: FeedSummary[];
    folders: FeedFolder[];
    onFoldersChanged: () => void | Promise<void>;
  }

  let { feeds, folders, onFoldersChanged }: Props = $props();

  let creatingFolder = $state(false);
  let newFolderName = $state('');
  let createError = $state<string | null>(null);
  let creating = $state(false);

  let renamingFolderId = $state<number | null>(null);
  let renameValue = $state('');
  let renameError = $state<string | null>(null);
  let renaming = $state(false);

  let confirmingDeleteFolderId = $state<number | null>(null);
  let deleteError = $state<string | null>(null);
  let deleting = $state(false);

  function feedCountInFolder(folderId: number): number {
    return feeds.filter((feed) => feed.folderId === folderId).length;
  }

  function isDuplicateFolderName(name: string, excludeId?: number): boolean {
    const normalized = name.trim().toLowerCase();
    return folders.some((f) => f.id !== excludeId && f.name.trim().toLowerCase() === normalized);
  }

  function friendlyFolderError(error: unknown, name: string): string {
    const raw = String(error);
    if (raw.includes('UNIQUE constraint')) return `A folder named "${name}" already exists`;
    if (raw.toLowerCase().includes('cannot be empty')) return 'Folder name cannot be empty';
    return 'Something went wrong. Please try again.';
  }

  function startCreateFolder() {
    creatingFolder = true;
    newFolderName = '';
    createError = null;
  }

  function cancelCreateFolder() {
    creatingFolder = false;
    newFolderName = '';
    createError = null;
  }

  async function submitCreateFolder() {
    const name = newFolderName.trim();
    if (!name || creating) return;
    if (isDuplicateFolderName(name)) {
      createError = `A folder named "${name}" already exists`;
      return;
    }
    creating = true;
    createError = null;
    try {
      await createFeedFolder(name);
      creatingFolder = false;
      newFolderName = '';
      await onFoldersChanged();
    } catch (error) {
      createError = friendlyFolderError(error, name);
    } finally {
      creating = false;
    }
  }

  function startRename(folder: FeedFolder) {
    renamingFolderId = folder.id;
    renameValue = folder.name;
    renameError = null;
  }

  function cancelRename() {
    renamingFolderId = null;
    renameValue = '';
    renameError = null;
  }

  async function submitRename(folder: FeedFolder) {
    const name = renameValue.trim();
    if (!name || renaming) return;
    if (name === folder.name) {
      cancelRename();
      return;
    }
    if (isDuplicateFolderName(name, folder.id)) {
      renameError = `A folder named "${name}" already exists`;
      return;
    }
    renaming = true;
    renameError = null;
    try {
      await renameFeedFolder(folder.id, name);
      renamingFolderId = null;
      await onFoldersChanged();
    } catch (error) {
      renameError = friendlyFolderError(error, name);
    } finally {
      renaming = false;
    }
  }

  function startDeleteConfirm(folder: FeedFolder) {
    confirmingDeleteFolderId = folder.id;
    deleteError = null;
  }

  function cancelDelete() {
    confirmingDeleteFolderId = null;
    deleteError = null;
  }

  async function confirmDelete(folder: FeedFolder) {
    if (deleting) return;
    deleting = true;
    deleteError = null;
    try {
      await deleteFeedFolder(folder.id);
      confirmingDeleteFolderId = null;
      await onFoldersChanged();
    } catch (error) {
      deleteError = String(error);
    } finally {
      deleting = false;
    }
  }

  function autofocus(node: HTMLInputElement) {
    node.focus();
  }
</script>

<div class="section-heading">
  <div>
    <h2 id="folders-title">Folders</h2>
    <p>Group feeds for faster triage in the sidebar.</p>
  </div>
  {#if !creatingFolder}
    <button type="button" class="new-folder" onclick={startCreateFolder}>+ New folder</button>
  {/if}
</div>

{#if creatingFolder}
  <form
    class="create-form"
    onsubmit={(event) => {
      event.preventDefault();
      submitCreateFolder();
    }}
  >
    <input
      type="text"
      bind:value={newFolderName}
      placeholder="Folder name"
      use:autofocus
      disabled={creating}
      onkeydown={(event) => {
        if (event.key === 'Escape') {
          event.preventDefault();
          cancelCreateFolder();
        }
      }}
    />
    <button type="submit" disabled={creating || !newFolderName.trim()}>
      {creating ? 'Creating…' : 'Create'}
    </button>
    <button type="button" class="cancel" onclick={cancelCreateFolder}>Cancel</button>
  </form>
  {#if createError}<p class="field-error">{createError}</p>{/if}
{/if}

{#if folders.length === 0}
  <div class="empty-placeholder">
    <span class="empty-placeholder-icon"><Icon name="empty" size={20} /></span>
    <h3>No folders yet</h3>
    <p>Create a folder to group related feeds together.</p>
  </div>
{:else}
  <ul class="folder-list">
    {#each folders as folder (folder.id)}
      <li class="folder-card">
        <div class="folder-row">
          {#if renamingFolderId === folder.id}
            <form
              class="rename-form"
              onsubmit={(event) => {
                event.preventDefault();
                submitRename(folder);
              }}
            >
              <input
                type="text"
                bind:value={renameValue}
                use:autofocus
                disabled={renaming}
                onkeydown={(event) => {
                  if (event.key === 'Escape') {
                    event.preventDefault();
                    cancelRename();
                  }
                }}
              />
              <button type="submit" disabled={renaming || !renameValue.trim()}>
                {renaming ? 'Saving…' : 'Save'}
              </button>
              <button type="button" class="cancel" onclick={cancelRename}>Cancel</button>
            </form>
          {:else}
            <div class="folder-identity">
              <span class="folder-name">{folder.name}</span>
              <span class="folder-count"
                >{feedCountInFolder(folder.id)} feed{feedCountInFolder(folder.id) === 1
                  ? ''
                  : 's'}</span
              >
            </div>
            {#if confirmingDeleteFolderId !== folder.id}
              <div class="folder-actions">
                <button type="button" onclick={() => startRename(folder)}>Rename</button>
                <button type="button" class="danger" onclick={() => startDeleteConfirm(folder)}
                  >Delete</button
                >
              </div>
            {/if}
          {/if}
        </div>
        {#if renameError && renamingFolderId === folder.id}<p class="field-error">
            {renameError}
          </p>{/if}
        {#if confirmingDeleteFolderId === folder.id}
          <div class="confirm-row">
            <span>Delete "{folder.name}"? Its feeds move to Unfiled.</span>
            <div class="confirm-actions">
              <button type="button" onclick={cancelDelete}>Cancel</button>
              <button
                type="button"
                class="confirm-remove"
                disabled={deleting}
                onclick={() => confirmDelete(folder)}
              >
                {deleting ? 'Deleting…' : 'Delete'}
              </button>
            </div>
          </div>
          {#if deleteError}<p class="field-error">{deleteError}</p>{/if}
        {/if}
      </li>
    {/each}
  </ul>
{/if}

<style>
  .section-heading {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 20px;
    margin-bottom: 14px;
  }

  h2 {
    margin: 0 0 5px;
    font-size: 1.05em;
  }

  .section-heading p {
    margin: 0;
    color: var(--color-text-secondary);
    font-size: 0.9em;
  }

  button {
    min-height: 34px;
    padding: 6px 10px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-bg);
    color: var(--color-text);
    font: inherit;
    font-size: 0.85em;
    cursor: pointer;
  }

  button:hover:not(:disabled) {
    background: var(--color-hover-bg);
  }

  button:disabled {
    opacity: 0.55;
    cursor: default;
  }

  .new-folder {
    flex-shrink: 0;
    background: var(--color-accent-solid);
    border-color: var(--color-accent-solid);
    color: var(--color-on-accent);
  }

  .new-folder:hover:not(:disabled) {
    background: var(--color-accent-solid);
    filter: brightness(1.05);
  }

  .create-form,
  .rename-form {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 14px;
  }

  .rename-form {
    flex: 1;
    margin-bottom: 0;
  }

  .create-form input,
  .rename-form input {
    min-width: 0;
    flex: 1;
    min-height: 34px;
    padding: 6px 9px;
    border: 1px solid var(--color-accent);
    border-radius: var(--radius-sm);
    background: var(--color-bg);
    color: var(--color-text);
    font: inherit;
    font-size: 0.85em;
  }

  .cancel {
    color: var(--color-text-secondary);
  }

  .field-error {
    margin: -6px 0 14px;
    color: var(--color-error);
    font-size: 0.8em;
    line-height: 1.4;
  }

  .folder-list {
    display: grid;
    gap: 10px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .folder-card {
    padding: 13px 15px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
  }

  .folder-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 14px;
  }

  .folder-identity {
    display: flex;
    min-width: 0;
    flex: 1;
    align-items: baseline;
    gap: 10px;
  }

  .folder-name {
    overflow: hidden;
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .folder-count {
    flex-shrink: 0;
    padding: 1px 8px;
    border-radius: var(--radius-pill);
    background: var(--color-hover-bg);
    color: var(--color-text-secondary);
    font-size: 0.78em;
    font-weight: 600;
  }

  .folder-actions {
    display: flex;
    flex-shrink: 0;
    gap: 6px;
  }

  .danger {
    color: var(--color-error);
  }
</style>
