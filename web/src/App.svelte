<script>
  import axios from 'axios';
  import { onMount } from 'svelte';

  import Dropdown from './lib/Dropdown.svelte';
  import Scale from "./lib/Scale.svelte";
  import WelcomeModal from "./lib/WelcomeModal.svelte";
  import ColourPicker from "./lib/ColourPicker.svelte";
  import SingleFrame from "./lib/SingleFrame.svelte";
  import LoginModal from "./lib/LoginModal.svelte"

  function slugify(s) {
    s = s.replace(/[^A-Za-z0-9]/g, "-")
    s = s.replace(/([a-z])([A-Z])/g, "$1-$2", s)
    return s.toLowerCase()
  }

  const fontSizes = [20, 36, 48, 64, 72, 96];

  function handleLoadError(err) {
    if (err.response.status === 401) {
      // Our login session must have expired, show the login modal again
      showLoginModal = true
    } else {
      // Well, that wasn't supposed to happen
      console.error("load error: %o", err);
      // Maybe this will help?
      document.location.reload();
    }
  }

  function setSkeleton(target) {
    axios.get(`/v1/${target.slug}`)
            .then(resp => {
              allAnimations = resp.data["animations"]
                      .sort((a, b) => a.name > b.name ? 1 : a.name < b.name ? -1 : 0)
                      .map(anim => ({
                        css_class: `${target.slug}-animations`,
                        id: `${target.slug}-animations-${slugify(anim["name"])}`,
                        ...anim
                      }))
              allSkins = resp.data["skins"]
                      .sort((a, b) => a.name > b.name ? 1 : a.name < b.name ? -1 : 0)
                      .map(skin => ({
                        css_class: `${target.slug}-skins`,
                        id: `${target.slug}-skins-${slugify(skin["name"])}`,
                        ...skin
                      }))

              selectedSkeleton = target
              selectedAnimation = allAnimations.find(a => target.default_animation === a.name)
              selectedSkins = allSkins.filter(s => target.default_skins.includes(s.name))
              features = target.features;
              scale = target.default_scale
            })
      .catch(handleLoadError);
  }

  function loginSuccessful() {
    showLoginModal = false
    initialLoad()
  }

  let selectedSkeleton = {}
  let selectedAnimation = ""
  let selectedSkins = []
  let spoilersEnabled = window.spoilersEnabled
  let authenticationRequired = window.authenticationRequired

  let showWelcomeModal = !authenticationRequired
  let showLoginModal = authenticationRequired

  let animation_filter = ""
  let skin_filter = ""
  let scale = 1.0
  let colours = {}
  let onlyHead = false
  let singleFrame = false
  let petpet = false
  let flip = false
  let top_text = ""
  let top_text_size = 0
  let bottom_text = ""
  let bottom_text_size = 0
  let singleFrameTimestamp = 0.0
  let activeCategoryMenu = ""
  let debounceTextTimer;

  let allSkeletons = [];
  let allAnimations = [];
  let allSkins = [];
  let allColours = {};
  let features = [];

  $: allCategories = Array.from(new Set(allSkeletons.map(s => s.category).filter(s => s != "None")).values())
  $: filteredAnimations = allAnimations.filter(a => a.name.toLowerCase().includes(animation_filter.toLowerCase()))
  $: filteredSkins = allSkins.filter(a => a.name.toLowerCase().includes(skin_filter.toLowerCase()))
  $: filteredColours = () => {
    if (Object.keys(allColours).length > 0) {
      let filtered = []
      for (const skinSet of allColours["skins"]) {
        for (const eachSkin of skinSet["skins"]) {
          if (selectedSkins.filter(selectedSkin => selectedSkin.name === eachSkin).length > 0) {
            filtered.push(...skinSet["sets"])
          }
        }
      }
      filtered.push(...allColours["global"])
      return filtered
    } else {
      return []
    }
  }

  function debounceText(event) {
    clearTimeout(debounceTextTimer)
    debounceTextTimer = setTimeout(() => {
      top_text = document.getElementById("top_text").value
      top_text_size = parseInt(document.getElementById("top_text_size").value)
      bottom_text = document.getElementById("bottom_text").value
      bottom_text_size = parseInt(document.getElementById("bottom_text_size").value)
    }, 500)
  }

  function doLogout(ev) {
    ev.preventDefault()
    axios.post("/logout").then(resp => document.location.reload())
  }

  function addSkin(skin) {
    if (selectedSkins.filter(s => s.name === skin.name).length === 0) {
      // Special case - if selecting "JustHead" for a Lamb skin, first remove
      // any other selected skins.  Mitigates
      // https://twitter.com/cultofthelamb/status/1598474349232918528
      if (skin.name === "JustHead") {
        selectedSkins.length = 0
      }
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

  async function initialLoad() {
    axios.get("/v1/").then(resp => {
      allSkeletons = resp.data.actors
      allSkeletons.sort((a, b) => a.name < b.name ? -1 : a.name > b.name ? 1 : 0)
      setSkeleton(allSkeletons.find(s => s.slug === "follower"))
      // If we're showing the login modal, we can hide it - we must have a working session
      if (showLoginModal) { showLoginModal = false }
    }).catch(handleLoadError)
    axios.get("/v1/follower/colours").then(resp => allColours = resp.data).catch(handleLoadError)
  }

  $: animationUrl = () => {
    if (selectedSkins.length === 0 || !selectedAnimation) { return "" }
    let params = [];
    let baseUrl = `/v1/${selectedSkeleton.slug}/${encodeURIComponent(selectedSkins[0].name)}`
    for (const additionalSkin of selectedSkins.slice(1)) {
      params.push(`add_skin=${encodeURIComponent(additionalSkin.name)}`)
    }
    if (scale !== 1) {
      if (scale === "128x128") {
        params.push("custom_size=discord128x128")
      } else {
        params.push(`scale=${scale}`)
      }
    }
    params.push(`animation=${selectedAnimation.name}`)
    if (selectedSkeleton.slug === "follower") {
      for (const [key, value] of Object.entries(colours)){
        // "last" is an unknown colour entry, but it doesn't seem to have any effect - just suppress it for now.
        // I haven't removed it from the json just in case it does turn out to be something.
        if (key !== "last") {
          params.push(`${key}=${encodeURIComponent(value)}`)
        }
      }
    }

    if (features.includes("only_head") && onlyHead) {
      params.push("only_head=true")
    }

    if (petpet) {
      params.push("petpet=true")
    }

    if (flip) {
      params.push("flip=horizontal")
    }

    if (top_text) {
      params.push(`top_text=${encodeURIComponent(top_text)}`)
      if (top_text_size !== 0) {
        params.push(`top_text_size=${top_text_size}`)
      }
    }

    if (bottom_text) {
      params.push(`bottom_text=${encodeURIComponent(bottom_text)}`)
      if (bottom_text_size !== 0) {
        params.push(`bottom_text_size=${bottom_text_size}`)
      }
    }

    if (singleFrame) {
      params.push("format=png")
      params.push(`start_time=${singleFrameTimestamp}`)
    }
    return baseUrl + "?" + params.join("&")
  }

  $: headUrl = () => {
    if (selectedSkins.length === 0) { return "" }
    if (!selectedSkeleton.slug) { return "" }
    let params = [];
    let baseUrl = `/v1/${selectedSkeleton.slug}/${encodeURIComponent(selectedSkins[0].name)}`
    for (const additionalSkin of selectedSkins.slice(1)) {
      params.push(`add_skin=${encodeURIComponent(additionalSkin.name)}`)
    }
    params.push(`scale=0.25`)
    params.push(`animation=${encodeURIComponent('Avatars/avatar-normal')}`)
    params.push("format=png")
    return baseUrl + "?" + params.join("&")
  }

  onMount(initialLoad)
</script>

<nav class="navbar py-2 px-2">
  <div id="navbar" class="navbar-menu is-active">
    <div class="navbar-start">
      <div class="navbar-item has-dropdown is-hoverable">
        <a class="navbar-link">
          {selectedSkeleton.name}
        </a>
        <div class="navbar-dropdown">
          {#each allSkeletons as skel}
            {#if skel.category === "None"}
            <a class="navbar-item" class:is-primary={selectedSkeleton.slug === skel.slug} on:click={()=> setSkeleton(skel)}>{skel.name}</a>
            {/if}
          {/each}

          {#each allCategories as category}
            <div class="nested dropdown" class:is-active={activeCategoryMenu === category}>
              <a class="dropdown-item" on:click={() => activeCategoryMenu = category}>
                {category} &rsaquo;
              </a>

              <div class="dropdown-menu">
                <div class="dropdown-content">
                  {#each allSkeletons as skel}
                    {#if skel.category === category}
                      <a class="dropdown-item"
                         class:is-primary={selectedSkeleton.slug === skel.slug}
                         on:click={() => { activeCategoryMenu = ""; setSkeleton(skel); }}
                         >{skel.name}</a>
                    {/if}
                  {/each}
                </div>
              </div>
            </div>
          {/each}
        </div>
      </div>
    </div>

    <div class="navbar-end">
      <a class="navbar-item button is-info mx-1" class:is-hidden={singleFrame} href={animationUrl() + "&format=gif&download=true"}>
        Download GIF
      </a>
      <a class="navbar-item button is-info mx-1" class:is-hidden={singleFrame} href={animationUrl() + "&format=apng&download=true"}>
        Download APNG
      </a>
      <a class="navbar-item button is-info mx-1" class:is-hidden={!singleFrame} href={animationUrl() + "&format=png&download=true"}>
        Download PNG
      </a>
      <form method="POST" action="/logout" on:submit={doLogout}>
        <button type="submit" class="navbar-item button is-danger mx-1 ml-3" class:is-hidden={!authenticationRequired}>Logout</button>
      </form>
    </div>
  </div>
</nav>

<section class="section">
  <div class="has-text-centered" style="min-height: 300px">
    <div class="box is-centered is-inline-block mb-5">
      {#key singleFrame ? "" : animationUrl}
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

    {#if selectedSkeleton.slug === "follower"}
      <p class="mt-4">
        <ColourPicker colours={filteredColours()} url={headUrl()} bind:value={colours}></ColourPicker>
      </p>
    {/if}

    {#if features.includes("only_head") }
      <p class="mt-4">
        <label class="checkbox">
          <input type="checkbox" bind:checked={onlyHead}>
          Only show head
        </label>
      </p>
    {/if}

      <SingleFrame
              bind:enabled={singleFrame}
              bind:timestamp={singleFrameTimestamp}
              duration={selectedAnimation ? selectedAnimation.duration : 0}></SingleFrame>

      <div>
        <label class="checkbox">
          <input type="checkbox" bind:checked={flip}>
          Flip horizontally
        </label>
      </div>

      <div>
        <label class="checkbox">
          <input type="checkbox" bind:checked={petpet}>
          Petpet (<a href="https://benisland.neocities.org/petpet/">original site</a>)
        </label>
      </div>

        <div class="columns">
          <div class="column is-10">
            <textarea class="input" placeholder="Top text" id="top_text" rows="2" on:input={debounceText}></textarea>
          </div>
          <div class="column is-2">
            <select class="input" id="top_text_size" on:input={debounceText}>
              {#each fontSizes as fontSize}
                {#if fontSize === 36}
                  <option selected value="36">36px</option>
                {:else}
                  <option value={fontSize}>{fontSize}px</option>
                {/if}
              {/each}
            </select>
          </div>
        </div>

        <div class="columns">
          <div class="column is-10">
            <textarea class="input" placeholder="Bottom text" id="bottom_text" rows="2" on:input={debounceText}></textarea>
          </div>
        <div class="column is-2">
          <select class="input" id="bottom_text_size" on:input={debounceText}>
            {#each fontSizes as fontSize}
              {#if fontSize === 36}
                <option selected value="36">36px</option>
              {:else}
                <option value={fontSize}>{fontSize}px</option>
              {/if}
            {/each}
          </select>
        </div>
      </div>
    </div>
  </div>
</section>

<WelcomeModal bind:visible={showWelcomeModal} bind:spoilersEnabled={spoilersEnabled}></WelcomeModal>
<LoginModal bind:visible={showLoginModal} on:login={loginSuccessful}></LoginModal>
