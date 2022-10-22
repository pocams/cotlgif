<script>
  import axios from 'axios';
  import { onMount } from 'svelte';

  import Dropdown from './lib/Dropdown.svelte';
  import Scale from "./lib/Scale.svelte";
  import Color from "./lib/Color.svelte";

  function slugify(s) {
    s = s.replace(/[^A-Za-z0-9]/g, "-")
    s = s.replace(/([a-z])([A-Z])/g, "$1-$2", s)
    return s.toLowerCase()
  }

  function setSkeleton(target) {
    skeleton = target
    if (target === "player") {
      selectedAnimation = {name: "idle"}
      selectedSkins = [{name: "Lamb"}]
    } else {
      selectedAnimation = {name: "idle"}
      selectedSkins = [{name: "Fox"}]
    }

    axios.get(`/v1/${skeleton}`)
            .then(resp => {
              allAnimations = resp.data["animations"]
                      .sort((a, b) => a.name > b.name ? 1 : a.name < b.name ? -1 : 0)
                      .map(anim => ({
                        css_class: `${skeleton}-animations`,
                        id: `${skeleton}-animations-${slugify(anim["name"])}`,
                        ...anim
                      }))
              allSkins = resp.data["skins"]
                      .sort((a, b) => a.name > b.name ? 1 : a.name < b.name ? -1 : 0)
                      .map(skin => ({
                        css_class: `${skeleton}-skins`,
                        id: `${skeleton}-skins-${slugify(skin["name"])}`,
                        ...skin
                      }))

              // Turn the default selectedAnimation and selectedSkins values into real objects
              selectedAnimation = allAnimations.find(a => a.name === selectedAnimation.name)
              selectedSkins = [allSkins.find(s => s.name === selectedSkins[0].name)]
            });
  }

  let skeleton
  let selectedAnimation;
  let selectedSkins = [];

  let animation_filter = ""
  let skin_filter = ""
  let scale = 1.0
  let color1 = "#eeeeee"
  let color2 = "#cccccc"
  let color3 = "#aaaaaa"

  let allAnimations = [];
  let allSkins = [];

  $: filteredAnimations = allAnimations.filter(a => a.name.toLowerCase().includes(animation_filter.toLowerCase()))
  $: filteredSkins = allSkins.filter(a => a.name.toLowerCase().includes(skin_filter.toLowerCase()))

  function addSkin(skin) {
    if (selectedSkins.filter(s => s.name === skin.name).length === 0) {
      selectedSkins.push(skin)
      selectedSkins = selectedSkins
    }
  }

  function removeSkin(skin) {
    let newSelected = selectedSkins.filter(s => s.name !== skin.name)
    if (newSelected.length > 0) {
      selectedSkins = newSelected
    }
  }

  $: animationUrl = () => {
    if (selectedSkins.length === 0 || !selectedAnimation) { return "" }
    let params = [];
    let baseUrl = `/v1/${skeleton}/${encodeURIComponent(selectedSkins[0].name)}`
    for (const additionalSkin of selectedSkins.slice(1)) {
      params.push(`add_skin=${encodeURIComponent(additionalSkin.name)}`)
    }
    params.push(`scale=${scale}`)
    params.push(`animation=${selectedAnimation.name}`)
    params.push("format=apng")
    if (skeleton === "follower") {
      params.push(`color1=${encodeURIComponent(color1)}`)
      params.push(`color2=${encodeURIComponent(color2)}`)
      params.push(`color3=${encodeURIComponent(color3)}`)
    }
    return baseUrl + "?" + params.join("&")
  }

  onMount(async () => {
    setSkeleton("follower")
  })
</script>

<nav class="navbar">
  <div id="navbarBasicExample" class="navbar-menu">
    <div class="navbar-start">
      <a class="navbar-item button mx-1 my-1" class:is-primary={skeleton === "player"} on:click={()=> setSkeleton("player")}>
        Player
      </a>
      <a class="navbar-item button mx-1 my-1" class:is-primary={skeleton === "follower"} on:click={() => setSkeleton("follower")}>
        Follower
      </a>
    </div>
  </div>
</nav>

<section class="section">
  <div class="has-text-centered" style="min-height: 300px">
    <div class="box is-centered is-inline-block mb-5">
      {#key animationUrl}
        <img src={animationUrl()}>
      {/key}
    </div>
  </div>
  <!--
    #{key} means "destroy and recreate whatever's in this block when this value changes".  This forces the animation
    to start playing immediately, instead of the default behaviour of playing the old animation until the new animation
    is completely loaded.
  -->
  <div class="columns">
    <div class="column">
      <div class="panel">

      {#each selectedSkins as skin}
        <div class="panel-block" on:click={removeSkin(skin)}>
          <div class="image is-48x48 mr-3 {skin.css_class}" id={skin.id}></div>
          <p class="media-content is-size-4 is-rounded">
            {skin.name}
          </p>
        </div>
      {/each}

      </div>
      <br>
      <Dropdown options={allSkins} let:option={option} on:selected={(event) => { addSkin(event.detail.option) }}></Dropdown>
    </div>

    <div class="column">
      <Dropdown options={allAnimations} let:option={option} on:selected={(event) => selectedAnimation = event.detail.option}>
        <div>
          <p>{option.name}</p>
          <div class="has-background-info is-inline-block" style="height: 12px; width: {option.duration * 5}%"></div>
          <div class="is-size-7 is-inline-block">{option.duration.toFixed(1)}s</div>
        </div>
      </Dropdown>
    </div>

    <div class="column">
      <Scale bind:value={scale}></Scale>

    {#if skeleton === "follower"}
      <p class="mt-4"></p>
      <Color bind:value={color1}></Color>
      <Color bind:value={color2}></Color>
      <Color bind:value={color3}></Color>
    {/if}
    </div>
  </div>
</section>
