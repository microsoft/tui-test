<script lang="ts">
  import type { TraceModel } from "../types.js";
  import PropertyList from "./PropertyList.svelte";
  let { metadata }: { metadata: TraceModel["metadata"] } = $props();
</script>

<h2>Session</h2><PropertyList id="session-properties" values={metadata.properties} />
<h2>Effective timeout defaults</h2><PropertyList id="timeout-properties" values={metadata.timeouts} />
<h2>Capture</h2><p id="retention" class="muted">{metadata.retention}</p>
<details><summary>Raw metadata</summary><pre id="runtime">{metadata.evidence}</pre></details>
