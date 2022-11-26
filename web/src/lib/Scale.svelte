<script>
    let options = [
        {"display": "0.3x", "value": 0.3},
        {"display": "0.5x", "value": 0.5},
        {"display": "1.0x", "value": 1},
        {"display": "1.5x", "value": 1.5},
        {"display": "2.0x", "value": 2},
        {"display": "3.0x", "value": 3},
        {"display": "128x128", "value": "128x128"}
    ]

    export let value = 1;
</script>

<div class="card">
    <div class="control">
        <div class="buttons has-addons">
            {#each options as option}
                <button class="button"
                        on:click={() => value = option.value}
                        class:is-selected={value === option.value}
                        class:is-info={value === option.value}>
                    {option.display}
                </button>
            {/each}
        </div>
    </div>
</div>
