<script lang="ts">
  import FeedList from '$lib/components/FeedList.svelte';
  import Timeline from '$lib/components/Timeline.svelte';
  import DetailView from '$lib/components/DetailView.svelte';
</script>

<div class="layout">
  <aside class="pane feed-list-pane">
    <FeedList />
  </aside>
  <main class="pane timeline-pane">
    <Timeline />
  </main>
  <aside class="pane detail-pane">
    <DetailView />
  </aside>
</div>

<style>
  .layout {
    display: grid;
    grid-template-columns: 200px 1fr 1fr;
    height: 100vh;
    width: 100%;
  }

  .pane {
    min-width: 0;
    min-height: 0;
  }

  .feed-list-pane {
    background: var(--color-bg-secondary);
    border-right: 1px solid var(--color-border);
  }

  .timeline-pane {
    background: var(--color-bg);
    border-right: 1px solid var(--color-border);
  }

  .detail-pane {
    background: var(--color-bg);
  }
</style>
