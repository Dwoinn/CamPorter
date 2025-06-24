<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/tauri';
  import { writable } from 'svelte/store';

  const logContent = writable('');
  const error = writable('');

  async function fetchLogs() {
    try {
      const logs = await invoke<string>('read_log_file');
      logContent.set(logs);
      error.set('');
    } catch (e) {
      error.set('Failed to load logs: ' + (e as Error).message);
      logContent.set('');
    }
  }

  onMount(() => {
    fetchLogs();
  });
</script>

<style>
  .log-viewer {
    background: #181818;
    color: #e0e0e0;
    font-family: monospace;
    padding: 1rem;
    border-radius: 8px;
    max-height: 60vh;
    overflow-y: auto;
    white-space: pre-wrap;
    word-break: break-all;
  }
  .error {
    color: #ff5555;
    margin-bottom: 1rem;
  }
  .refresh-btn {
    margin-bottom: 1rem;
    background: #333;
    color: #fff;
    border: none;
    padding: 0.5rem 1rem;
    border-radius: 4px;
    cursor: pointer;
  }
  .refresh-btn:hover {
    background: #444;
  }
</style>

<div>
  <button class="refresh-btn" on:click={fetchLogs}>Refresh Logs</button>
  {#if $error}
    <div class="error">{$error}</div>
  {/if}
  <div class="log-viewer">
    {$logContent}
  </div>
</div>