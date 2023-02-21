<script>
  import { createEventDispatcher } from 'svelte';
  import axios from 'axios';

	const dispatch = createEventDispatcher();

  export let visible

  let theInput
  let password = ""
  let loading = false
  let error

  $: if (visible) {
    if (theInput) {
      theInput.focus()
    }
  }

  function login(ev) {
    ev.preventDefault()
    if (loading) { return }
    error = null
    loading = true

    axios.post("/login", {password})
      .then(resp => {
        dispatch("login", {});
        loading = false
        password = ""
      })
      .catch(err => {
        console.log("login error: %o", err)
        error = err.response.data.error
        loading = false
        password = ""
      })
  }
</script>

<div class="modal" class:is-active={visible}>
  <div class="modal-background"></div>
  <div class="modal-content">
    <section class="modal-card-head">
      <h1>Authentication required</h1>
    </section>
    <section class="modal-card-body">

      {#if error}
      <article class="message is-danger" >
        <div class="message-body">
          Login error: {error}
        </div>
      </article>
      {/if}

      <form class="columns" on:submit={login}>
        <div class="column is-10">
          <input class="input is-normal" type="password" placeholder="Password" bind:value={password} bind:this={theInput}>
        </div>
        <div class="column">
          <button type="submit" class="button is-primary" class:is-loading={loading}>Login</button>
        </div>
      </form>
    </section>
  </div>
</div>
