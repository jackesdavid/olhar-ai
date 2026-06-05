<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  type Delivery = { target: string; chat_id: number; at: string; ok: boolean };
  type Alert = {
    id: number;
    level: "debug" | "info" | "warn" | "error";
    message: string;
    room_id: number | null;
    room_name: string | null;
    source: string;
    created_at: string;
    deliveries: Delivery[];
  };

  let alerts = $state<Alert[]>([]);
  let error = $state("");
  let loading = $state(true);

  async function load() {
    loading = true;
    error = "";
    try {
      // Calls the Rust `list_alerts` command (Bishop's alerts model) over Tauri IPC.
      alerts = await invoke<Alert[]>("list_alerts");
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    load();
  });

  const levelColor: Record<string, string> = {
    debug: "#64748b",
    info: "#2563eb",
    warn: "#d97706",
    error: "#dc2626",
  };
  const fmt = (iso: string) => {
    try {
      return new Date(iso).toLocaleString();
    } catch {
      return iso;
    }
  };
</script>

<main>
  <header>
    <div>
      <h1>Alerts</h1>
      <p class="sub">
        Phase-0a spike — data served by a Rust <code>list_alerts</code> command (Bishop's
        alerts model) over Tauri&nbsp;IPC, rendered in SvelteKit. Neutral, non-Alien styling
        stands in for shadcn-svelte.
      </p>
    </div>
    <div class="actions">
      <a class="link" href="/map3d">Floor plan 3D →</a>
      <a class="link" href="/live">Live camera →</a>
      <button class="refresh" onclick={load} disabled={loading}>
        {loading ? "Loading…" : "Refresh"}
      </button>
    </div>
  </header>

  {#if error}
    <div class="note error">Command error: {error}</div>
  {:else if loading}
    <div class="note">Loading…</div>
  {:else if alerts.length === 0}
    <div class="note">No alerts found (looked for <code>alerts.json</code>).</div>
  {:else}
    <ul class="list">
      {#each alerts as a (a.id)}
        <li class="card">
          <span class="badge" style="background:{levelColor[a.level] ?? '#64748b'}">
            {a.level.toUpperCase()}
          </span>
          <div class="body">
            <div class="msg">{a.message}</div>
            <div class="meta">
              <span>#{a.id}</span>
              <span>{a.source}</span>
              {#if a.room_name}<span>{a.room_name}</span>{/if}
              <span>{fmt(a.created_at)}</span>
              {#if a.deliveries.length}
                <span class="ok">delivered ×{a.deliveries.length}</span>
              {/if}
            </div>
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</main>

<style>
  :global(body) {
    margin: 0;
    background: #0b0e14;
    color: #e6e9ef;
    font-family: Inter, system-ui, -apple-system, Segoe UI, Roboto, sans-serif;
  }
  main {
    max-width: 760px;
    margin: 0 auto;
    padding: 32px 20px 56px;
  }
  header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
    margin-bottom: 20px;
  }
  h1 {
    margin: 0 0 6px;
    font-size: 22px;
    font-weight: 650;
    letter-spacing: -0.01em;
  }
  .sub {
    margin: 0;
    max-width: 56ch;
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
  .actions {
    flex: none;
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .link {
    padding: 8px 12px;
    border-radius: 8px;
    border: 1px solid #2a3242;
    color: #cdd3df;
    text-decoration: none;
    font-size: 13px;
    font-weight: 550;
  }
  .link:hover {
    border-color: #3a4256;
    background: #161b26;
  }
  .refresh {
    flex: none;
    padding: 8px 14px;
    border-radius: 8px;
    border: 1px solid #2a3242;
    background: #161b26;
    color: #e6e9ef;
    font-size: 13px;
    font-weight: 550;
    cursor: pointer;
  }
  .refresh:hover {
    border-color: #3a4256;
    background: #1b2130;
  }
  .refresh:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .note {
    padding: 14px 16px;
    border: 1px solid #2a3242;
    border-radius: 10px;
    color: #97a0b3;
    font-size: 14px;
  }
  .note.error {
    border-color: #5b2330;
    background: #2a151b;
    color: #f4a8b3;
  }
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .card {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 14px 16px;
    border: 1px solid #222a38;
    border-radius: 12px;
    background: #11151f;
  }
  .badge {
    flex: none;
    min-width: 56px;
    text-align: center;
    padding: 4px 8px;
    border-radius: 999px;
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.03em;
    color: #fff;
  }
  .body {
    min-width: 0;
  }
  .msg {
    font-size: 15px;
    font-weight: 500;
    margin-bottom: 4px;
  }
  .meta {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 12px;
    font-size: 12px;
    color: #8b95a7;
  }
  .meta .ok {
    color: #4ade80;
  }
</style>
