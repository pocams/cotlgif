<script>
    export let enabled = false;
    export let timestamp = 0.0;
    export let duration = 0.0;

    let debounceTimer;

    $: if (!enabled) { timestamp = 0.0 }

    function debounce(event) {
        clearTimeout(debounceTimer)
        debounceTimer = setTimeout(() => timestamp = parseFloat(event.target.value), 100)
    }
</script>

<div class="card">
    <label class="checkbox">
        <input type="checkbox" bind:checked={enabled}>
        Render a still image
    </label>
    <div class="control is-fullwidth">
        <button class="button is-small" disabled={!enabled || (timestamp <= 0.0)} on:click={() => timestamp -= 0.01}>&laquo;</button>
        <input type="range" style="width: 60%" disabled={!enabled} min="0" max={duration} step="0.01" value={timestamp} on:change={debounce}>
        <button class="button is-small" disabled={!enabled || (timestamp >= duration)} on:click={() => timestamp += 0.01}>&raquo;</button>
        Frame <span class:has-text-grey-light={!enabled}>{typeof(timestamp) === "number" ? (timestamp / (1/50) + 1).toFixed(0) : "??"}</span>
    </div>
</div>
