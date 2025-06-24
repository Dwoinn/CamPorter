<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { emit } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';

  export let file: {
    name: string;
    path: string;
    extension: string;
    is_image: boolean;
    is_video: boolean;
  };
  export let sourcePath: string; // Absolute path to load

  import { invoke } from '@tauri-apps/api/core';
  import { readFile, BaseDirectory } from '@tauri-apps/plugin-fs';
  import Plyr from 'plyr';
  import 'plyr/dist/plyr.css';

  let blobUrl: string | null = null;
  let overlayEl: HTMLDivElement | null = null;
  let isLoading = true;

  async function loadMediaBlob() {
    isLoading = true;
    console.log('Starting media preview load for', file?.name, 'from', sourcePath);
    if (!sourcePath) {
      console.error('No sourcePath provided for preview');
      return;
    }
    try {
      // Step 1: Copy to temp dir
      console.log('Invoking copy_to_temp for', sourcePath);
      const tempPath = await invoke<string>('copy_to_temp', { filePath: sourcePath });
      console.log('File copied to temp path:', tempPath);
      // Step 2: Read from temp dir
      // const data = await invoke<Uint8Array>('plugin:fs|read_file', { path: tempPath, options: { encoding: null } });
      
      const data = await readFile(file?.name, {baseDir: BaseDirectory.Temp});

      console.log('Read file from temp path, byte length:', data?.length);
      const ext = file.extension.toLowerCase();
      let mime = '';
      if (file.is_image) {
        if (['jpg', 'jpeg'].includes(ext)) mime = 'image/jpeg';
        else if (ext === 'png') mime = 'image/png';
        else if (ext === 'gif') mime = 'image/gif';
        else if (ext === 'webp') mime = 'image/webp';
        else mime = 'image/*';
      } else if (file.is_video) {
        if (ext === 'mp4') mime = 'video/mp4';
        else if (ext === 'webm') mime = 'video/webm';
        else if (ext === 'mov') mime = 'video/quicktime';
        else mime = 'video/*';
      }
      const blob = new Blob([new Uint8Array(data)], { type: mime });
      if (blobUrl) URL.revokeObjectURL(blobUrl);
      blobUrl = URL.createObjectURL(blob);
      console.log('Created blobUrl for preview:', blobUrl);
    } catch (e) {
      console.error('Failed to load media file:', e);
      blobUrl = null;
    } finally {
      isLoading = false;
    }
  }

/* Duplicate onMount import and block removed. See above for the main onMount block. */

// Reload media when file or sourcePath changes
$: if (file && sourcePath) {
  loadMediaBlob();
}

  let isFullscreen = true;
  let isVideoPaused = true;
  let showNavBar = true;
  let navBarTimeout: ReturnType<typeof setTimeout> | null = null;
  let zoom = 1;
  let offsetX = 0;
  let offsetY = 0;
  let lastPanX = 0;
  let lastPanY = 0;
  let isPanning = false;
  let lastTouchDistance = 0;
  let lastTouchCenter = { x: 0, y: 0 };

  async function closePreview() {
    try {
      await getCurrentWindow().close();
    } catch (e) {
      window.close();
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      closePreview();
    }
    if (file.is_video && (e.key === ' ' || e.key === 'Spacebar')) {
      togglePlayPause();
      e.preventDefault();
    }
  }

  function handleMediaAction() {
    if (file.is_video) {
      togglePlayPause();
    }
  }
  function handleClick(e: MouseEvent) {
    handleMediaAction();
  }

  function togglePlayPause() {
    const video = document.getElementById('preview-video') as HTMLVideoElement;
    if (video) {
      if (video.paused) {
        video.play();
        isVideoPaused = false;
      } else {
        video.pause();
        isVideoPaused = true;
      }
      showNavBar = true;
      resetNavBarTimeout();
    }
  }

  function handleMouseMove() {
    if (file.is_video) {
      showNavBar = true;
      resetNavBarTimeout();
    }
  }

  function resetNavBarTimeout() {
    if (navBarTimeout) clearTimeout(navBarTimeout);
    if (!isVideoPaused) {
      navBarTimeout = setTimeout(() => {
        showNavBar = false;
      }, 2000);
    }
  }

  function handleWheel(e: WheelEvent) {
    e.preventDefault();
    const rect = (e.target as HTMLElement).getBoundingClientRect();
    const mouseX = e.clientX - rect.left;
    const mouseY = e.clientY - rect.top;
    const prevZoom = zoom;
    const delta = e.deltaY < 0 ? 0.1 : -0.1;
    zoom = Math.max(0.2, Math.min(zoom + delta, 5));
    // Adjust offset so zoom is centered at mouse position
    offsetX = (offsetX - mouseX) * (zoom / prevZoom) + mouseX;
    offsetY = (offsetY - mouseY) * (zoom / prevZoom) + mouseY;
  }

  function handleTouchStart(e: TouchEvent) {
    if (e.touches.length === 1) {
      isPanning = true;
      lastPanX = e.touches[0].clientX;
      lastPanY = e.touches[0].clientY;
    } else if (e.touches.length === 2) {
      isPanning = false;
      const dx = e.touches[0].clientX - e.touches[1].clientX;
      const dy = e.touches[0].clientY - e.touches[1].clientY;
      lastTouchDistance = Math.sqrt(dx * dx + dy * dy);
      lastTouchCenter = {
        x: (e.touches[0].clientX + e.touches[1].clientX) / 2,
        y: (e.touches[0].clientY + e.touches[1].clientY) / 2
      };
    }
  }
  
  function handleTouchMove(e: TouchEvent) {
    if (e.touches.length === 1 && isPanning) {
      const dx = e.touches[0].clientX - lastPanX;
      const dy = e.touches[0].clientY - lastPanY;
      offsetX += dx;
      offsetY += dy;
      lastPanX = e.touches[0].clientX;
      lastPanY = e.touches[0].clientY;
    } else if (e.touches.length === 2) {
      const dx = e.touches[0].clientX - e.touches[1].clientX;
      const dy = e.touches[0].clientY - e.touches[1].clientY;
      const dist = Math.sqrt(dx * dx + dy * dy);
      const center = {
        x: (e.touches[0].clientX + e.touches[1].clientX) / 2,
        y: (e.touches[0].clientY + e.touches[1].clientY) / 2
      };
      const prevZoom = zoom;
      zoom = Math.max(0.2, Math.min(zoom * (dist / lastTouchDistance), 5));
      // Adjust offset so zoom is centered at pinch center
      offsetX = (offsetX - center.x) * (zoom / prevZoom) + center.x;
      offsetY = (offsetY - center.y) * (zoom / prevZoom) + center.y;
      lastTouchDistance = dist;
      lastTouchCenter = center;
    }
  }
  
  function handleTouchEnd(e: TouchEvent) {
    if (e.touches.length === 0) {
      isPanning = false;
    }
  }

  // (Removed duplicate import and merged onMount logic above)

  onDestroy(() => {
    document.body.style.overflow = '';
    window.removeEventListener('keydown', handleKeydown);
    if (navBarTimeout) clearTimeout(navBarTimeout);
  });
</script>

<style>
  .preview-overlay {
    position: fixed;
    z-index: 9999;
    top: 0; left: 0; right: 0; bottom: 0;
    background: rgba(0,0,0,0.98);
    display: flex;
    align-items: center;
    justify-content: center;
    width: 100vw; height: 100vh;
    overflow: hidden;
    animation: fadeIn 0.2s;
  }
  @keyframes fadeIn {
    from { opacity: 0; }
    to { opacity: 1; }
  }
  .close-btn {
    position: absolute;
    top: 32px;
    right: 48px;
    z-index: 10001;
    background: rgba(0,0,0,0.7);
    color: #fff;
    border: none;
    border-radius: 50%;
    width: 48px; height: 48px;
    font-size: 28px;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    opacity: 1;
    transition: opacity 0.2s;
  }
  /* .nav-bar removed: no longer used */
  .media-container {
    max-width: 100vw;
    max-height: 100vh;
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
    position: relative;
    width: 100vw;
    height: 100vh;
    touch-action: none;
  }
  .preview-image {
    max-width: 100vw;
    max-height: 100vh;
    transform: scale(var(--zoom, 1));
    transition: transform 0.2s;
    user-select: none;
    pointer-events: auto;
    background: #111;
    border-radius: 8px;
    box-shadow: 0 0 32px #000a;
  }
</style>

<div
  class="preview-overlay"
  role="region"
  tabindex="0"
  on:mousemove={handleMouseMove}
  on:wheel={handleWheel}
  on:touchstart={handleTouchStart}
  on:touchmove={handleTouchMove}
  on:touchend={handleTouchEnd}
  on:keydown={handleKeydown}
>
  {#if showNavBar}
    <button class="close-btn" on:click={closePreview} title="Close (Esc)">✕</button>
  {/if}
  <div
    class="media-container"
    role="region"
    tabindex="0"
    on:click={handleClick}
    on:keydown={e => { if (e.key === 'Enter' || e.key === ' ') handleMediaAction(); }}
    on:mousedown={(e) => {
      if (e.button === 0) {
        isPanning = true;
        lastPanX = e.clientX;
        lastPanY = e.clientY;
      }
    }}
    on:mousemove={(e) => {
      if (isPanning) {
        const dx = e.clientX - lastPanX;
        const dy = e.clientY - lastPanY;
        offsetX += dx;
        offsetY += dy;
        lastPanX = e.clientX;
        lastPanY = e.clientY;
      }
    }}
    on:mouseup={() => { isPanning = false; }}
    on:mouseleave={() => { isPanning = false; }}
    style="touch-action: none;"
  >
    {#if isLoading}
      <div style="display:flex;flex-direction:column;align-items:center;justify-content:center;width:100%;height:100%;">
        <svg width="48" height="48" viewBox="0 0 48 48" fill="none" style="display:block;margin:auto;">
          <circle cx="24" cy="24" r="20" stroke="#007bff" stroke-width="4" stroke-linecap="round" stroke-dasharray="31.4 31.4" stroke-dashoffset="0">
            <animateTransform attributeName="transform" type="rotate" from="0 24 24" to="360 24 24" dur="1s" repeatCount="indefinite"/>
          </circle>
        </svg>
        <div style="text-align:center;margin-top:1em;color:white;">Loading media...</div>
      </div>
    {:else if blobUrl && file.is_image}
      <img
        src={blobUrl}
        alt={file.name}
        class="preview-image"
        style="transform: translate({offsetX}px, {offsetY}px) scale({zoom});"
        draggable="false"
        on:load={() => { isLoading = false; }}
      />
    {:else if blobUrl && file.is_video}
      <div style="width:100vw;height:100vh;display:flex;align-items:center;justify-content:center;">
        <video
          id="plyr-video"
          src={blobUrl}
          class="preview-video"
          style="width:100vw;height:100vh;max-width:100vw;max-height:100vh;transform: translate({offsetX}px, {offsetY}px) scale({zoom});"
          autoplay
          controls
          on:loadeddata={() => { isLoading = false; }}
        ></video>
      </div>
    {/if}
  </div>
</div>