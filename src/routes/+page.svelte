<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from '@tauri-apps/api/event';
  import { open } from '@tauri-apps/plugin-dialog';
  import { onMount, tick } from 'svelte';

  interface Drive {
    name: string;
    mount_point: string;
    device_id: string;
  }

  interface MediaFile {
    name: string;
    path: string;
    size: number;
    modified: number;
    extension: string;
    is_image: boolean;
    is_video: boolean;
  }

  let drives: Drive[] = [];
  let selectedDrive = '';
  let destination = '';
  let mediaFiles: MediaFile[] = [];
  let selectedFiles: Set<string> = new Set();
  let progress = '';
  let progressPercent = 0;
  let isLoading = false;
  let isImporting = false;
  let existingFiles: Set<string> = new Set();
  let thumbnailCache: Record<string, string> = {};
  let thumbnailLoadingStates: Record<string, 'pending' | 'loading' | 'loaded' | 'error'> = {};
  let thumbnailGenerationQueue: string[] = [];
  let isGeneratingThumbnails = false;
  let forceUpdate = 0; // Simple counter to force reactivity

  onMount(async () => {
    await refreshDrives();
    await loadSavedDestination();
  });

  async function loadSavedDestination() {
    try {
      const savedPath = await invoke('load_destination_path');
      if (savedPath) {
        destination = savedPath as string;
      }
    } catch (err) {
      console.error('Error loading saved destination:', err);
    }
  }

  async function saveDestination() {
    if (destination) {
      try {
        await invoke('save_destination_path', { path: destination });
      } catch (err) {
        console.error('Error saving destination:', err);
      }
    }
  }

  async function selectDestinationFolder() {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: 'Select Destination Folder'
      });
      
      if (selected) {
        destination = selected as string;
        await saveDestination();
      }
    } catch (err) {
      console.error('Error selecting folder:', err);
    }
  }

  async function refreshDrives() {
    try {
      drives = await invoke('list_removable_drives');
    } catch (err) {
      console.error('Error listing drives:', err);
    }
  }

  async function loadMediaFiles() {
    if (!selectedDrive) return;
    
    isLoading = true;
    try {
      mediaFiles = await invoke('list_media_files', { drivePath: selectedDrive });
      selectedFiles.clear();
      selectedFiles = selectedFiles; // Trigger reactivity
      
      // Initialize thumbnail states and start generation
      initializeThumbnailGeneration();
    } catch (err) {
      console.error('Error loading files:', err);
      mediaFiles = [];
    } finally {
      isLoading = false;
    }
  }

  function toggleFileSelection(filePath: string) {
    if (selectedFiles.has(filePath)) {
      selectedFiles.delete(filePath);
    } else {
      selectedFiles.add(filePath);
    }
    selectedFiles = selectedFiles; // Trigger reactivity
  }

  function selectAllFiles() {
    selectedFiles = new Set(mediaFiles.map(f => f.path));
  }

  function clearSelection() {
    selectedFiles.clear();
    selectedFiles = selectedFiles;
  }

  async function importSelectedFiles() {
    if (selectedFiles.size === 0 || !destination) {
      return;
    }

    isImporting = true;
    progressPercent = 0;
    progress = '';

    try {
      // Set up progress listener
      const unlisten = await listen('import-progress', (event) => {
        const message = event.payload as string;
        
        if (message.startsWith('PROGRESS_BYTES:')) {
          const parts = message.split(':');
          if (parts.length === 3) {
            const copied = parseInt(parts[1]);
            const total = parseInt(parts[2]);
            progressPercent = total > 0 ? (copied / total) * 100 : 0;
          }
        } else if (message.startsWith('PROGRESS:')) {
          const parts = message.split(':');
          if (parts.length === 3) {
            progressPercent = (parseInt(parts[1]) / parseInt(parts[2])) * 100;
          }
        } else {
          progress = message;
        }
      });
      
      // Convert selected files to array
      const filesToImport = Array.from(selectedFiles);
      
      // Start import
      await invoke('import_selected_files', { 
        filePaths: filesToImport,
        targetPath: destination
      });
      
      progress = 'Import completed successfully';
      unlisten();
    } catch (err) {
      progress = `Import failed: ${err}`;
    } finally {
      isImporting = false;
    }
  }

  async function unmountDrive() {
    if (!selectedDrive) return;

    try {
      await invoke('unmount_drive', { mountPoint: selectedDrive });
      mediaFiles = [];
      selectedFiles.clear();
      selectedFiles = selectedFiles;
      selectedDrive = '';
      await refreshDrives();
    } catch (err) {
      console.error('Unmount failed:', err);
      progress = `Unmount failed: ${err}`;
    }
  }

  async function openDestinationFolder() {
    if (!destination) return;
    
    try {
      await invoke('open_destination_folder', { path: destination });
    } catch (err) {
      console.error('Error opening destination folder:', err);
    }
  }

  async function checkExistingFiles() {
    if (!destination || mediaFiles.length === 0) return;
    
    try {
      const filePaths = mediaFiles.map(f => f.path);
      const response = await invoke('check_files_exist_in_destination', {
        filePaths,
        destinationPath: destination
      });
      
      // Safely cast the response
      const results = Array.isArray(response) ? response as boolean[] : [];
      
      existingFiles.clear();
      for (let i = 0; i < results.length; i++) {
        if (results[i] === true) {
          existingFiles.add(mediaFiles[i].path);
        }
      }
      existingFiles = existingFiles; // Trigger reactivity
    } catch (err) {
      console.error('Error checking existing files:', err);
    }
  }

  function formatFileSize(bytes: number): string {
    const sizes = ['B', 'KB', 'MB', 'GB'];
    if (bytes === 0) return '0 B';
    const i = Math.floor(Math.log(bytes) / Math.log(1024));
    return Math.round(bytes / Math.pow(1024, i) * 100) / 100 + ' ' + sizes[i];
  }

  function formatDate(timestamp: number): string {
    return new Date(timestamp * 1000).toLocaleDateString();
  }

  function initializeThumbnailGeneration() {
    // Clear previous states
    thumbnailCache = {};
    thumbnailLoadingStates = {};
    thumbnailGenerationQueue = [];
    forceUpdate = 0;
    
    // Initialize all files as pending
    mediaFiles.forEach(file => {
      if (file.is_image || file.is_video) {
        thumbnailLoadingStates[file.path] = 'pending';
        thumbnailGenerationQueue.push(file.path);
      }
    });
    
    // Start generating thumbnails
    startThumbnailGeneration();
  }

  async function startThumbnailGeneration() {
    if (isGeneratingThumbnails || thumbnailGenerationQueue.length === 0) return;
    
    isGeneratingThumbnails = true;
    
    // Process one thumbnail at a time with proper UI updates
    processNextThumbnail();
  }

  async function processNextThumbnail() {
    if (thumbnailGenerationQueue.length === 0) {
      isGeneratingThumbnails = false;
      return;
    }
    
    const filePath = thumbnailGenerationQueue.shift()!;
    await generateSingleThumbnail(filePath);
    
    // Allow Svelte to update the DOM
    await tick();
    
    // Schedule next thumbnail generation
    setTimeout(() => {
      processNextThumbnail();
    }, 50); // Increased delay to ensure UI updates
  }

  async function generateSingleThumbnail(filePath: string) {
    if (thumbnailCache[filePath]) return;
    
    // Set loading state
    thumbnailLoadingStates[filePath] = 'loading';
    forceUpdate++; // Trigger reactivity
    
    try {
      const thumbnail = await invoke('get_file_thumbnail', { filePath });
      thumbnailCache[filePath] = thumbnail as string;
      thumbnailLoadingStates[filePath] = 'loaded';
      forceUpdate++; // Trigger reactivity
    } catch (err) {
      console.error('Error loading thumbnail:', err);
      thumbnailLoadingStates[filePath] = 'error';
      forceUpdate++; // Trigger reactivity
    }
  }

  async function getThumbnail(filePath: string): Promise<string> {
    if (thumbnailCache[filePath]) {
      return thumbnailCache[filePath];
    }

    try {
      const thumbnail = await invoke('get_file_thumbnail', { filePath });
      thumbnailCache[filePath] = thumbnail as string;
      thumbnailCache = thumbnailCache; // Trigger reactivity
      return thumbnail as string;
    } catch (err) {
      console.error('Error loading thumbnail:', err);
      return '';
    }
  }

  function getThumbnailState(filePath: string): 'pending' | 'loading' | 'loaded' | 'error' {
    return thumbnailLoadingStates[filePath] || 'pending';
  }

  function getThumbnailFromCache(filePath: string): string | null {
    return thumbnailCache[filePath] || null;
  }

  // Force reactivity when thumbnail states change
  $: thumbnailData = { states: thumbnailLoadingStates, cache: thumbnailCache, force: forceUpdate };

  $: if (selectedDrive) {
    loadMediaFiles();
  }

  // Check existing files when destination or media files change
  $: if (destination && mediaFiles.length > 0) {
    checkExistingFiles();
  }
</script>

<style>
  :global(.app-container) {
    display: flex;
    flex-direction: row;
    height: 100vh;
    width: 100vw;
    background: #f8f9fa;
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
    top: 0;
    left: 0;
  }
  
  .logo-container {
    display: flex;
    align-items: center;
    margin-bottom: 16px;
  }

  .app-logo {
    width: 150px;
    height: 150px;
    margin-right: auto;
    margin-left: auto;
  }

  .sidebar {
    width: 320px;
    min-width: 320px;
    max-width: 320px;
    background: white;
    border-right: 1px solid #e9ecef;
    padding: 24px;
    overflow-y: auto;
    box-shadow: 2px 0 8px rgba(0,0,0,0.05);
    flex-shrink: 0;
  }
  

  .main-content {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  h1 {
    margin: 0 0 32px 0;
    color: #212529;
    font-size: 28px;
    font-weight: 700;
  }

  h2 {
    margin: 0 0 12px 0;
    color: #495057;
    font-size: 14px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .section {
    margin-bottom: 32px;
  }

  .no-devices {
    color: #6c757d;
    font-size: 14px;
    margin: 0 0 12px 0;
  }

  .device-select {
    width: 100%;
    padding: 12px 16px;
    border: 2px solid #e9ecef;
    border-radius: 8px;
    margin-bottom: 12px;
    font-size: 14px;
    transition: border-color 0.2s ease;
  }

  .device-select:focus {
    outline: none;
    border-color: #007bff;
  }

  .destination-container {
    display: flex;
    gap: 8px;
    margin-bottom: 12px;
  }

  .destination-input {
    flex: 1;
    padding: 12px 16px;
    border: 2px solid #e9ecef;
    border-radius: 8px;
    font-size: 14px;
    transition: border-color 0.2s ease;
  }

  .destination-input:focus {
    outline: none;
    border-color: #007bff;
  }

  .btn-folder {
    background: #f8f9fa;
    border: 2px solid #e9ecef;
    border-radius: 8px;
    padding: 12px 16px;
    cursor: pointer;
    font-size: 18px;
    transition: all 0.2s ease;
    min-width: 52px;
  }

  .btn-folder:hover {
    background: #e9ecef;
    border-color: #007bff;
  }

  .btn-primary {
    background: #007bff;
    color: white;
    border: none;
    padding: 12px 20px;
    border-radius: 8px;
    cursor: pointer;
    font-weight: 600;
    font-size: 14px;
    transition: background-color 0.2s ease;
  }

  .btn-primary:hover:not(:disabled) {
    background: #0056b3;
  }

  .btn-secondary {
    background: #6c757d;
    color: white;
    border: none;
    padding: 8px 16px;
    border-radius: 6px;
    cursor: pointer;
    font-size: 12px;
    font-weight: 500;
    transition: background-color 0.2s ease;
  }

  .btn-secondary:hover:not(:disabled) {
    background: #545b62;
  }

  .btn-danger {
    background: #dc3545;
    color: white;
    border: none;
    padding: 12px 20px;
    border-radius: 8px;
    cursor: pointer;
    font-weight: 600;
    font-size: 14px;
    transition: background-color 0.2s ease;
  }

  .btn-danger:hover:not(:disabled) {
    background: #c82333;
  }

  .full-width {
    width: 100%;
  }

  button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .progress-container {
    position: relative;
    margin-bottom: 8px;
  }

  progress {
    width: 100%;
    height: 8px;
    border-radius: 4px;
  }

  .progress-text {
    position: absolute;
    right: 0;
    top: -2px;
    font-size: 12px;
    color: #495057;
    font-weight: 600;
  }

  .status-text {
    font-size: 12px;
    color: #6c757d;
    margin: 8px 0 0 0;
  }

  .empty-state, .loading-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    text-align: center;
    color: #6c757d;
    padding: 40px;
  }

  .empty-icon {
    font-size: 64px;
    margin-bottom: 24px;
    opacity: 0.5;
  }

  .loading-spinner {
    width: 40px;
    height: 40px;
    border: 4px solid #e9ecef;
    border-left: 4px solid #007bff;
    border-radius: 50%;
    animation: spin 1s linear infinite;
    margin-bottom: 24px;
  }

  @keyframes spin {
    0% { transform: rotate(0deg); }
    100% { transform: rotate(360deg); }
  }

  .file-browser-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 20px 24px;
    background: white;
    border-bottom: 1px solid #e9ecef;
  }

  .file-count {
    color: #495057;
    font-weight: 500;
  }

  .selection-controls {
    display: flex;
    gap: 8px;
  }

  .file-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
    gap: 16px;
    padding: 24px;
    overflow-y: auto;
    background: #f8f9fa;
  }

  .file-card {
    background: white;
    border: 2px solid #e9ecef;
    border-radius: 12px;
    padding: 16px;
    cursor: pointer;
    transition: all 0.2s ease;
    position: relative;
  }

  .file-card:hover {
    border-color: #007bff;
    box-shadow: 0 4px 12px rgba(0,123,255,0.15);
    transform: translateY(-2px);
  }

  .file-card.selected {
    border-color: #007bff;
    background: #f0f8ff;
    box-shadow: 0 4px 12px rgba(0,123,255,0.25);
  }

  .file-preview {
    display: flex;
    justify-content: center;
    align-items: center;
    height: 120px;
    margin-bottom: 12px;
    background: #f8f9fa;
    border-radius: 8px;
    position: relative;
  }

  .file-thumbnail {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    width: 100%;
    position: relative;
  }

  .file-thumbnail.loading {
    opacity: 0.6;
  }

  .thumbnail-image {
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: 8px;
  }

  .thumbnail-video {
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: 8px;
  }

  .file-icon {
    font-size: 48px;
    opacity: 0.7;
  }

  .video-indicator {
    position: absolute;
    bottom: 8px;
    right: 8px;
    background: rgba(0,0,0,0.7);
    color: white;
    padding: 2px 6px;
    border-radius: 4px;
    font-size: 10px;
    font-weight: bold;
  }

  .file-info {
    text-align: center;
  }

  .file-name {
    font-weight: 500;
    font-size: 13px;
    color: #212529;
    margin-bottom: 8px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  

  .file-meta {
    font-size: 11px;
    color: #6c757d;
    display: flex;
    justify-content: space-between;
  }

  .selection-indicator {
    position: absolute;
    top: 12px;
    right: 12px;
    background: #007bff;
    color: white;
    border-radius: 50%;
    width: 28px;
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 14px;
    font-weight: bold;
    box-shadow: 0 2px 4px rgba(0,0,0,0.2);
  }

  .existing-indicator {
    position: absolute;
    top: 12px;
    left: 12px;
    background: #28a745;
    color: white;
    border-radius: 50%;
    width: 28px;
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 16px;
    font-weight: bold;
    box-shadow: 0 2px 4px rgba(0,0,0,0.2);
  }

  .thumbnail-loading-indicator {
    position: absolute;
    bottom: 8px;
    left: 8px;
    width: 16px;
    height: 16px;
    border: 2px solid #e9ecef;
    border-left: 2px solid #007bff;
    border-radius: 50%;
    animation: spin 1s linear infinite;
    background: rgba(255, 255, 255, 0.9);
  }

  .file-thumbnail.loading .file-icon {
    opacity: 0.5;
  }
</style>

<div class="app-container">
  <!-- Left Sidebar -->
  <div class="sidebar">
    <div class="logo-container">
      <img src="/camporter-logo.png" alt="CamPorter Logo" class="app-logo" />
      <!-- <h1>CamPorter</h1> -->
    </div>
    
    <!-- Device Selection -->
    <div class="section">
      <h2>Device</h2>
      {#if drives.length === 0}
        <p class="no-devices">No devices found</p>
        <button on:click={refreshDrives} class="btn-secondary">Refresh</button>
      {:else}
        <select bind:value={selectedDrive} class="device-select">
          <option value="">Select a device</option>
          {#each drives as drive}
            <option value={drive.mount_point}>
              {drive.name}
            </option>
          {/each}
        </select>
        <button on:click={refreshDrives} class="btn-secondary">Refresh</button>
      {/if}
    </div>

    <!-- Unmount -->
    {#if selectedDrive}
    <div class="section">
      <button on:click={unmountDrive} class="btn-danger full-width">
        Unmount Device
      </button>
    </div>
    {/if}

    <!-- Destination -->
    <div class="section">
      <h2>Destination</h2>
      <div class="destination-container">
        <input
          type="text"
          bind:value={destination}
          on:blur={saveDestination}
          placeholder="/home/user/Pictures"
          class="destination-input"
        />
        <button on:click={selectDestinationFolder} class="btn-folder" title="Browse for folder">
          📁
        </button>
      </div>
      {#if destination}
        <button on:click={openDestinationFolder} class="btn-secondary full-width" style="margin-top: 8px;">
          📂 Open Destination Folder
        </button>
      {/if}
    </div>

    <!-- Copy Action -->
    <div class="section">
      <button 
        on:click={importSelectedFiles} 
        disabled={selectedFiles.size === 0 || !destination || isImporting}
        class="btn-primary full-width"
      >
        {#if isImporting}
          Copying...
        {:else}
          Copy Selected ({selectedFiles.size})
        {/if}
      </button>
    </div>

    <!-- Progress -->
    {#if isImporting || progressPercent > 0}
    <div class="section">
      <h2>Progress</h2>
      <div class="progress-container">
        <progress value={progressPercent} max="100"></progress>
        <div class="progress-text">{Math.round(progressPercent)}%</div>
      </div>
      {#if progress}
        <p class="status-text">{progress}</p>
      {/if}
    </div>
    {/if}
  </div>

  <!-- Main Content Area -->
  <div class="main-content">
    {#if !selectedDrive}
      <div class="empty-state">
        <div class="empty-icon">📷️</div>
        <h2>Select a Device</h2>
        <p>Choose a connected device from the sidebar to browse its media files.</p>
      </div>
    {:else if isLoading}
      <div class="loading-state">
        <div class="loading-spinner"></div>
        <h2>Loading Files...</h2>
        <p>Scanning device for media files...</p>
      </div>
    {:else if mediaFiles.length === 0}
      <div class="empty-state">
        <div class="empty-icon">📁</div>
        <h2>No Media Files Found</h2>
        <p>The selected device doesn't contain any supported media files.</p>
      </div>
    {:else}
      <!-- File Browser Header -->
      <div class="file-browser-header">
        <div class="file-count">
          {mediaFiles.length} files found
        </div>
        <div class="selection-controls">
          <button on:click={selectAllFiles} class="btn-secondary">
            Select All
          </button>
          <button on:click={clearSelection} disabled={selectedFiles.size === 0} class="btn-secondary">
            Clear Selection
          </button>
        </div>
      </div>

      <!-- File Grid -->
      <div class="file-grid">
        {#each mediaFiles as file}
          <div
            class="file-card {selectedFiles.has(file.path) ? 'selected' : ''}"
            on:click={() => toggleFileSelection(file.path)}
            on:keydown={(e) => {
              if (e.key === 'Enter' || e.key === ' ') {
                toggleFileSelection(file.path);
                e.preventDefault();
              }
            }}
            role="button"
            tabindex="0"
            aria-pressed={selectedFiles.has(file.path)}
          >
            <div class="file-preview">
              {#if file.is_image}
                {#key `${file.path}-${forceUpdate}`}
                  {#if thumbnailLoadingStates[file.path] === 'loaded' && thumbnailCache[file.path]}
                    <img src={thumbnailCache[file.path]} alt={file.name} class="thumbnail-image" />
                  {:else}
                    <div class="file-thumbnail {thumbnailLoadingStates[file.path] === 'loading' ? 'loading' : ''}">
                      <div class="file-icon image-icon">🖼️</div>
                      {#if thumbnailLoadingStates[file.path] === 'loading'}
                        <div class="thumbnail-loading-indicator"></div>
                      {/if}
                    </div>
                  {/if}
                {/key}
              {:else if file.is_video}
                {#key `${file.path}-${forceUpdate}`}
                  {#if thumbnailLoadingStates[file.path] === 'loaded' && thumbnailCache[file.path]}
                    <video class="thumbnail-video" muted>
                      <source src={thumbnailCache[file.path]} type="video/mp4">
                      <div class="file-icon video-icon">🎥</div>
                    </video>
                    <div class="video-indicator">VIDEO</div>
                  {:else}
                    <div class="file-thumbnail {thumbnailLoadingStates[file.path] === 'loading' ? 'loading' : ''}">
                      <div class="file-icon video-icon">🎥</div>
                      <div class="video-indicator">VIDEO</div>
                      {#if thumbnailLoadingStates[file.path] === 'loading'}
                        <div class="thumbnail-loading-indicator"></div>
                      {/if}
                    </div>
                  {/if}
                {/key}
              {:else}
                <div class="file-thumbnail">
                  <div class="file-icon audio-icon">🎵</div>
                </div>
              {/if}
            </div>
            
            <div class="file-info">
              <div class="file-name" title={file.name}>{file.name}</div>
              <div class="file-meta">
                <span class="file-size">{formatFileSize(file.size)}</span>
                <span class="file-date">{formatDate(file.modified)}</span>
              </div>
            </div>
            
            {#if selectedFiles.has(file.path)}
              <div class="selection-indicator">✓</div>
            {/if}
            
            {#if existingFiles.has(file.path)}
              <div class="existing-indicator" title="File already exists in destination">✅</div>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>


