<script lang="ts">
  import { onMount, tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { goto } from "$app/navigation";
  import { toast } from "svelte-sonner";

  import Send from "@lucide/svelte/icons/send";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Bot from "@lucide/svelte/icons/bot";
  import User from "@lucide/svelte/icons/user";
  import Video from "@lucide/svelte/icons/video";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import Mic from "@lucide/svelte/icons/mic";
  import Volume2 from "@lucide/svelte/icons/volume-2";
  import VolumeX from "@lucide/svelte/icons/volume-x";

  import { Button } from "$lib/components/ui/button";
  import EmptyState from "$lib/components/empty-state.svelte";

  type ChatMsg =
    | { kind: "user"; text: string }
    | { kind: "bishop"; text: string }
    | { kind: "image"; camera: string; data: string };

  let messages = $state<ChatMsg[]>([]);
  let input = $state("");
  let sending = $state(false);
  let ready = $state(true);
  let liveIp = $state<string | null>(null);

  // ---- voice (TTS) + dictation (STT) ---------------------------------------
  let voiceReady = $state(false);
  let speakReplies = $state(false);
  let recording = $state(false);
  let transcribing = $state(false);
  let mediaRecorder: MediaRecorder | null = null;
  let chunks: Blob[] = [];

  let scroller = $state<HTMLDivElement | null>(null);

  async function scrollToBottom() {
    await tick();
    if (scroller) scroller.scrollTop = scroller.scrollHeight;
  }

  onMount(() => {
    invoke<boolean>("chat_ready").then((r) => (ready = r)).catch(() => (ready = false));
    invoke<boolean>("voice_ready").then((r) => (voiceReady = r)).catch(() => (voiceReady = false));

    // Tool side-effects streamed from the backend during a turn.
    const unImg = listen<{ camera: string; data: string }>("chat-image", (e) => {
      messages = [...messages, { kind: "image", camera: e.payload.camera, data: e.payload.data }];
      scrollToBottom();
    });
    const unLive = listen<string>("chat-open-live", (e) => {
      liveIp = e.payload;
    });

    return () => {
      unImg.then((un) => un());
      unLive.then((un) => un());
    };
  });

  async function send() {
    const text = input.trim();
    if (!text || sending) return;
    messages = [...messages, { kind: "user", text }];
    input = "";
    liveIp = null;
    sending = true;
    await scrollToBottom();
    try {
      const reply = await invoke<string>("send_chat", { text });
      if (reply) {
        messages = [...messages, { kind: "bishop", text: reply }];
        if (speakReplies && voiceReady) speakText(reply);
      }
    } catch (err) {
      toast.error("Sentinel", { description: String(err) });
      messages = [...messages, { kind: "bishop", text: `⚠️ ${String(err)}` }];
    } finally {
      sending = false;
      await scrollToBottom();
    }
  }

  async function reset() {
    try {
      await invoke("reset_chat");
    } catch (err) {
      console.error("reset_chat failed", err);
    }
    messages = [];
    liveIp = null;
  }

  function onKeydown(e: KeyboardEvent) {
    // Enter sends; Shift+Enter inserts a newline.
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      send();
    }
  }

  // ---- text-to-speech ------------------------------------------------------
  async function speakText(text: string) {
    try {
      const b64 = await invoke<string>("speak", { text });
      const audio = new Audio(`data:audio/mpeg;base64,${b64}`);
      audio.play().catch(() => {});
    } catch (err) {
      console.error("speak failed", err);
    }
  }

  // ---- dictation (mic → Scribe → send) -------------------------------------
  function bytesToBase64(bytes: Uint8Array): string {
    let bin = "";
    const chunk = 0x8000;
    for (let i = 0; i < bytes.length; i += chunk) {
      bin += String.fromCharCode(...bytes.subarray(i, i + chunk));
    }
    return btoa(bin);
  }

  async function toggleMic() {
    if (recording) {
      mediaRecorder?.stop();
      return;
    }
    if (!voiceReady) return;
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      chunks = [];
      const mr = new MediaRecorder(stream);
      mediaRecorder = mr;
      mr.ondataavailable = (e) => {
        if (e.data.size > 0) chunks.push(e.data);
      };
      mr.onstop = async () => {
        stream.getTracks().forEach((t) => t.stop());
        recording = false;
        const blob = new Blob(chunks, { type: mr.mimeType || "audio/webm" });
        await transcribeAndSend(blob);
      };
      mr.start();
      recording = true;
    } catch (err) {
      toast.error("Microphone unavailable", { description: String(err) });
    }
  }

  async function transcribeAndSend(blob: Blob) {
    if (blob.size === 0) return;
    transcribing = true;
    try {
      const buf = new Uint8Array(await blob.arrayBuffer());
      const audioB64 = bytesToBase64(buf);
      const transcript = await invoke<string>("transcribe", { audioB64, mime: blob.type });
      if (transcript) {
        input = transcript;
        await send();
      }
    } catch (err) {
      toast.error("Couldn't transcribe that", { description: String(err) });
    } finally {
      transcribing = false;
    }
  }
</script>

<div class="flex h-[calc(100vh-3.5rem)] flex-col">
  <div class="flex items-center justify-between border-b border-border px-1 pb-4">
    <div>
      <h1 class="text-2xl font-semibold tracking-tight">Chat</h1>
      <p class="text-sm text-muted-foreground">
        Talk to Sentinel — it can act on your cameras, alerts, and recipients.
      </p>
    </div>
    <div class="flex items-center gap-2">
      {#if voiceReady}
        <Button
          variant={speakReplies ? "default" : "outline"}
          size="sm"
          title={speakReplies ? "Sentinel's replies are spoken" : "Speak Sentinel's replies"}
          onclick={() => (speakReplies = !speakReplies)}
        >
          {#if speakReplies}<Volume2 class="size-4" />{:else}<VolumeX class="size-4" />{/if}
          Voice
        </Button>
      {/if}
      <Button variant="outline" size="sm" onclick={reset} disabled={messages.length === 0 || sending}>
        <Trash2 class="size-4" />
        Reset
      </Button>
    </div>
  </div>

  {#if !ready}
    <div class="mt-4 flex items-start gap-3 rounded-md border border-amber-500/30 bg-amber-500/10 p-4 text-sm">
      <TriangleAlert class="mt-0.5 size-5 shrink-0 text-amber-400" />
      <div>
        <p class="font-medium text-amber-300">Sentinel is offline</p>
        <p class="text-muted-foreground">
          Set <code class="rounded bg-background px-1 py-0.5 font-mono">ANTHROPIC_API_KEY</code> in
          the environment and restart to enable the assistant.
        </p>
      </div>
    </div>
  {/if}

  <!-- Message log -->
  <div bind:this={scroller} class="min-h-0 flex-1 space-y-4 overflow-y-auto px-1 py-4">
    {#if messages.length === 0}
      <div class="flex h-full items-center justify-center">
        <EmptyState
          title="Talk to Sentinel"
          description="Ask it to list your cameras, check what's on one, raise an alert, or notify someone."
        />
      </div>
    {/if}

    {#each messages as m, i (i)}
      {#if m.kind === "image"}
        <div class="flex justify-start">
          <div class="max-w-[80%] space-y-1">
            <p class="flex items-center gap-1.5 text-xs text-muted-foreground">
              <Video class="size-3.5" />
              {m.camera}
            </p>
            <img
              src={`data:image/jpeg;base64,${m.data}`}
              alt={`Snapshot of ${m.camera}`}
              class="rounded-lg border border-border"
            />
          </div>
        </div>
      {:else}
        <div class="flex gap-3" class:flex-row-reverse={m.kind === "user"}>
          <div
            class="flex size-8 shrink-0 items-center justify-center rounded-full border border-border"
            class:bg-primary={m.kind === "user"}
            class:text-primary-foreground={m.kind === "user"}
            class:bg-muted={m.kind === "bishop"}
          >
            {#if m.kind === "user"}
              <User class="size-4" />
            {:else}
              <Bot class="size-4" />
            {/if}
          </div>
          <div
            class="max-w-[80%] whitespace-pre-wrap rounded-lg px-3.5 py-2 text-sm leading-relaxed"
            class:bg-primary={m.kind === "user"}
            class:text-primary-foreground={m.kind === "user"}
            class:bg-muted={m.kind === "bishop"}
          >
            {m.text}
          </div>
        </div>
      {/if}
    {/each}

    {#if sending}
      <div class="flex gap-3">
        <div class="flex size-8 shrink-0 items-center justify-center rounded-full border border-border bg-muted">
          <Bot class="size-4" />
        </div>
        <div class="flex items-center gap-2 rounded-lg bg-muted px-3.5 py-2 text-sm text-muted-foreground">
          <LoaderCircle class="size-4 animate-spin" />
          Thinking…
        </div>
      </div>
    {/if}

    {#if liveIp}
      <div class="flex justify-start">
        <Button variant="outline" size="sm" onclick={() => goto(`/live?cam=${liveIp}`)}>
          <Video class="size-4" />
          Open live feed
        </Button>
      </div>
    {/if}
  </div>

  <!-- Composer -->
  <div class="border-t border-border pt-4">
    <div class="flex items-end gap-2">
      <textarea
        bind:value={input}
        onkeydown={onKeydown}
        rows="1"
        placeholder={ready ? "Message Sentinel…" : "Set ANTHROPIC_API_KEY to chat"}
        disabled={!ready || sending}
        class="flex max-h-40 min-h-10 w-full resize-none rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-sm placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50"
      ></textarea>
      {#if voiceReady}
        <Button
          variant={recording ? "destructive" : "outline"}
          onclick={toggleMic}
          disabled={!ready || sending || transcribing}
          title={recording ? "Stop and transcribe" : "Dictate a message"}
        >
          {#if transcribing}
            <LoaderCircle class="size-4 animate-spin" />
          {:else}
            <Mic class="size-4 {recording ? 'animate-pulse' : ''}" />
          {/if}
        </Button>
      {/if}
      <Button onclick={send} disabled={!ready || sending || input.trim().length === 0}>
        {#if sending}
          <LoaderCircle class="size-4 animate-spin" />
        {:else}
          <Send class="size-4" />
        {/if}
      </Button>
    </div>
    <p class="mt-1.5 px-1 text-xs text-muted-foreground">
      Enter to send · Shift+Enter for a new line{#if voiceReady} · mic to dictate{/if}
    </p>
  </div>
</div>
