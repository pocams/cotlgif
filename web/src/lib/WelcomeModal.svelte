<script>
    export let visible
    export let spoilersEnabled = false
</script>

<div class="modal" class:is-active={visible}>
    <div class="modal-background"></div>
    <div class="modal-content">
        <section class="modal-card-body is-size-5">
            <p>This is fan made and not an official Cult of the Lamb product.</p>
            <p>All images and animations are the property of Massive Monster.</p>
            <hr>

            {#if spoilersEnabled}
                <p>This tool includes spoilers for unreleased Cult of the Lamb content. Some
                    of the animations will be broken and not render properly.</p>
                <p class="mt-3">If you are not working on a Cult of the Lamb mod, you should:</p>
                <a class="is-size-3" href="http://cotl.xl0.org">Use the proper version</a>
                <p class="mt-3">If you continue anyway, Ratau will be disappointed in you.</p>
                <img class="image mx-auto" src="/v1/ratau/normal?animation=knucklebones/lose-game-loop&scale=0.6">
            {:else}
                <p>This tool includes spoilers for all Cult of the Lamb gameplay.</p>
                <p>I've done my best to exclude content that isn't part of the game, but if
                    you see anything I missed, please let me know.</p>
            {/if}
            <hr>
            <p>
                <a class="is-size-6" href="https://discord.com/invite/massivemonster">pocams#0058 on Massive Monster discord</a>
            </p>
            <p>
                <a class="is-size-6" href="https://github.com/pocams/cotlgif">Source on Github</a>
            </p>
            <p class="is-size-6 mt-4">Sorry the UI is terrible.</p>
        </section>
        <footer class="modal-card-foot">
            <button class="button is-info is-fullwidth" on:click={() => visible = false}>Okay</button>
        </footer>
    </div>
</div>
