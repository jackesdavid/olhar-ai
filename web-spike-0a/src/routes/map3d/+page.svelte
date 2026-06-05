<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { Canvas } from "@threlte/core";
  import Scene from "$lib/three/Scene.svelte";

  type RoomCamera = { ip: string; dx: number; dy: number };
  type Room = {
    id: number;
    name: string;
    x: number;
    y: number;
    w: number;
    h: number;
    cameras: RoomCamera[];
  };

  let rooms = $state<Room[]>([]);
  let selected = $state<string | null>(null);
  let error = $state("");

  async function load() {
    try {
      rooms = await invoke<Room[]>("list_rooms");
    } catch (e) {
      error = String(e);
    }
  }
  $effect(() => {
    load();
  });

  const camCount = $derived(rooms.reduce((n, r) => n + r.cameras.length, 0));
</script>

<div class="wrap">
  <header>
    <div>
      <h1>Floor plan — 3D</h1>
      <p class="sub">
        Phase-0c spike — Bishop's real <code>map.json</code> rooms rendered as 3D objects with
        <strong>Threlte</strong> (Three.js). Drag to orbit · scroll to zoom · click a room to
        select. Cyan cones are camera markers. (GLB "real models" slot in via <code>useGltf</code>.)
      </p>
    </div>
    <a class="back" href="/">← Alerts</a>
  </header>

  <div class="status">
    <span>{rooms.length} rooms · {camCount} cameras</span>
    <span class="sel">{selected ? `Selected: ${selected}` : "Click a room"}</span>
  </div>
  {#if error}<div class="err">Command error: {error}</div>{/if}

  <div class="stage">
    <Canvas>
      <Scene {rooms} bind:selected />
    </Canvas>
  </div>
</div>

<style>
  .wrap {
    max-width: 1000px;
    margin: 0 auto;
    padding: 28px 20px 40px;
  }
  header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
    margin-bottom: 14px;
  }
  h1 {
    margin: 0 0 6px;
    font-size: 22px;
    font-weight: 650;
  }
  .sub {
    margin: 0;
    max-width: 70ch;
    font-size: 13px;
    line-height: 1.5;
    color: #97a0b3;
  }
  code {
    background: #1b2130;
    border-radius: 5px;
    padding: 1px 5px;
    font-size: 0.85em;
  }
  .back {
    flex: none;
    padding: 8px 14px;
    border-radius: 8px;
    border: 1px solid #2a3242;
    background: #161b26;
    color: #e6e9ef;
    font-size: 13px;
    text-decoration: none;
  }
  .status {
    display: flex;
    justify-content: space-between;
    font-size: 13px;
    color: #8b95a7;
    margin-bottom: 10px;
  }
  .status .sel {
    color: #f59e0b;
    font-weight: 600;
  }
  .err {
    padding: 12px 14px;
    border: 1px solid #5b2330;
    background: #2a151b;
    color: #f4a8b3;
    border-radius: 10px;
    margin-bottom: 10px;
  }
  .stage {
    height: 62vh;
    min-height: 380px;
    border: 1px solid #222a38;
    border-radius: 12px;
    overflow: hidden;
    background: #070a11;
  }
</style>
