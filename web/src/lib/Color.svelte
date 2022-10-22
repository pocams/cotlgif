<script>
    let debounceTimer

    export let value = "#dddddd";

    function debounce(event) {
        clearTimeout(debounceTimer)
        debounceTimer = setTimeout(() => value = event.target.value, 100)
    }
</script>


<div class="card is-inline-block" style="min-width: 32%">
    <div class="control">
        <input class="input" type="color" on:input={debounce} value={value}>
    </div>
</div>
