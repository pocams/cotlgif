<script>
    let debounceTimer

    export let colours = []
    export let url = ""
    export let value = {}

    let rows = []

    $: {
        let newRows = [];
        for (let i = 0; i < colours.length; i += 8) {
            newRows.push(colours.slice(i, i+8))
        }
        rows = newRows
    }

    function urlFor(colourSet) {
        // "last" is an unknown colour entry, but it doesn't seem to have any effect - just suppress it for now.
        // I haven't removed it from the json just in case it does turn out to be something.
        let params = Object.entries(colourSet).filter(([k, _]) => k !== "last").map(([k, v]) => `${k}=${encodeURIComponent(v)}`)
        return url + "&" + params.join("&")
    }

    function debounce(event) {
        clearTimeout(debounceTimer)
        debounceTimer = setTimeout(() => value = event.target.value, 100)
    }
</script>

<div class="tile is-ancestor is-vertical">
    {#each rows as row}
        <div class="tile is-parent">
        {#each row as colourSet}
        <div class="tile is-child" on:click={() => value = colourSet}>
            <img src="{urlFor(colourSet)}">
        </div>
        {/each}
        </div>
    {/each}
</div>
