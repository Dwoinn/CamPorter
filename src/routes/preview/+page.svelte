<script lang="ts">
  import MediaPreview from '../../components/MediaPreview.svelte';
  import { page } from '$app/stores';
  import { get } from 'svelte/store';

  // Read file info from query string
  const params = get(page).url.searchParams;
  const name = params.get('name') || '';
  const path = params.get('path') || '';
  const extension = params.get('extension') || '';
  const is_image = params.get('is_image') === 'true';
  const is_video = params.get('is_video') === 'true';

  const file = { name, path, extension, is_image, is_video };
</script>

<MediaPreview {file} sourcePath={path} />