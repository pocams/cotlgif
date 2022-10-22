<script>
    import { createEventDispatcher } from 'svelte';

    const dispatch = createEventDispatcher();

    function selected(option) {
        dispatch("selected", {option: option})
    }

    export let options = [];
    export let multiple = false;

    let open = false;
    let filterString = "";
    $: filteredOptions = options.filter(o => o.name.toLowerCase().includes(filterString.toLowerCase()))
</script>

<div class="panel">
    <div class="panel-heading">
        <div class="control">
            <input class="input is-medium" placeholder="Search" bind:value={filterString}>
        </div>
    </div>

{#each filteredOptions as option}
    <div class="panel-block" on:click={() => selected(option)}>
        <div class="image is-48x48 mr-3 {option.css_class}" id={option.id}></div>
        <slot option={option} >
            {option.name}
        </slot>
    </div>
{/each}

</div>