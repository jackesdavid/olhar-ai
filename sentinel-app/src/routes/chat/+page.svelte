<script lang="ts">
  // The conversational assistant, backed by the cloud control plane (Claude + the agent
  // tool loop, ElevenLabs TTS/STT). It can list cameras, capture snapshots, describe what's
  // visible, manage alerts, and notify recipients — all scoped to the signed-in tenant.
  import { tick } from "svelte";
  import { toast } from "svelte-sonner";

  import Bot from "@lucide/svelte/icons/bot";
  import SendIcon from "@lucide/svelte/icons/send";
  import Mic from "@lucide/svelte/icons/mic";
  import Square from "@lucide/svelte/icons/square";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Volume2 from "@lucide/svelte/icons/volume-2";
  import VolumeX from "@lucide/svelte/icons/volume-x";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import Camera from "@lucide/svelte/icons/camera";

  import { Button } from "$lib/components/ui/button";
  import { agent, type ChatTurn, type ChatToolEvent } from "$lib/api";

  type Msg = { role: "user" | "assistant"; content: string; tools?: ChatToolEvent[] };

  let messages = $state<Msg[]>([]);
  let input = $state("");
  let busy = $state(false);
  let recording = $state(false);
  let speakReplies = $state(true);

  let scroller = $state<HTMLDivElement | null>(null);
  let recorder: MediaRecorder | null = null;
  let chunks: Blob[] = [];
  let player: HTMLAudioElement | null = null;

  function canSend(): boolean {
    return !busy && input.trim().length > 0;
  }

  function bubbleClass(role: Msg["role"]): string {
    return role === "user"
      ? "ml-auto bg-primary text-primary-foreground"
      : "mr-auto border border-border bg-card text-card-foreground";
  }

  function printToneClass(level: string): string {
    if (level === "error") return "text-destructive";
    if (level === "warn") return "text-amber-500";
    return "text-muted-foreground";
  }

  async function scrollToEnd() {
    await tick();
    scroller?.scrollTo({ top: scroller.scrollHeight, behavior: "smooth" });
  }

  async function send() {
    if (!canSend()) return;
    const text = input.trim();
    input = "";
    messages.push({ role: "user", content: text });
    void scrollToEnd();

    const history: ChatTurn[] = messages.map((m) => ({ role: m.role, content: m.content }));
    busy = true;
    try {
      const reply = await agent.chat(history);
      messages.push({ role: "assistant", content: reply.text, tools: reply.tool_events });
      void scrollToEnd();
      if (speakReplies && reply.text.trim()) void speak(reply.text);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "The assistant didn't respond.");
    } finally {
      busy = false;
    }
  }

  async function speak(text: string) {
    try {
      const blob = await agent.tts(text);
      if (!blob) return; // voice not configured — stay silent
      player?.pause();
      const url = URL.createObjectURL(blob);
      player = new Audio(url);
      player.onended = () => URL.revokeObjectURL(url);
      await player.play();
    } catch {
      // Autoplay may be blocked until a user gesture; failing silently is fine.
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void send();
    }
  }

  async function toggleMic() {
    if (recording) {
      recorder?.stop();
      return;
    }
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      chunks = [];
      recorder = new MediaRecorder(stream);
      recorder.ondataavailable = (e) => e.data.size > 0 && chunks.push(e.data);
      recorder.onstop = () => {
        stream.getTracks().forEach((t) => t.stop());
        recording = false;
        void transcribe();
      };
      recorder.start();
      recording = true;
    } catch {
      toast.error("Couldn't access the microphone.");
    }
  }

  async function transcribe() {
    if (chunks.length === 0) return;
    const blob = new Blob(chunks, { type: recorder?.mimeType || "audio/webm" });
    busy = true;
    try {
      const text = await agent.stt(blob);
      input = input ? `${input} ${text}` : text;
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Transcription failed.");
    } finally {
      busy = false;
    }
  }
</script>

<div class="mx-auto flex h-[calc(100vh-7rem)] max-w-3xl flex-col">
  <div class="flex items-start justify-between gap-3 pb-4">
    <div>
      <h1 class="text-2xl font-semibold tracking-tight">Assistant</h1>
      <p class="text-sm text-muted-foreground">Ask Sentinel about your cameras, alerts, and people.</p>
    </div>
    <Button
      variant="ghost"
      size="icon"
      title={speakReplies ? "Mute spoken replies" : "Speak replies aloud"}
      onclick={() => (speakReplies = !speakReplies)}
    >
      {#if speakReplies}<Volume2 class="size-4" />{:else}<VolumeX class="size-4" />{/if}
    </Button>
  </div>

  <div bind:this={scroller} class="flex-1 space-y-4 overflow-y-auto rounded-xl border border-border bg-muted/20 p-4">
    {#if messages.length === 0}
      <div class="flex h-full flex-col items-center justify-center gap-3 text-center text-muted-foreground">
        <Bot class="size-10" />
        <p class="max-w-sm text-sm">
          Try "list my cameras", "what's on the front door?", or "show me recent alerts".
        </p>
      </div>
    {/if}

    {#each messages as m (m)}
      <div class="flex flex-col gap-2">
        <div class={`max-w-[85%] whitespace-pre-wrap rounded-2xl px-3.5 py-2 text-sm leading-relaxed ${bubbleClass(m.role)}`}>
          {m.content}
        </div>

        {#if m.tools}
          {#each m.tools as ev (ev)}
            {#if ev.type === "snapshot" || ev.type === "image"}
              <figure class="mr-auto max-w-[85%] overflow-hidden rounded-xl border border-border bg-card">
                <img src={ev.image} alt={ev.type === "snapshot" ? ev.camera : ev.label} class="w-full" />
                <figcaption class="flex items-center gap-1.5 px-3 py-1.5 text-xs text-muted-foreground">
                  <Camera class="size-3" />
                  {ev.type === "snapshot" ? ev.camera : ev.label}
                </figcaption>
              </figure>
            {:else if ev.type === "open_live"}
              <a
                href="/live"
                class="interactive mr-auto inline-flex items-center gap-1.5 rounded-lg border border-border bg-card px-3 py-1.5 text-xs font-medium text-primary"
              >
                <ExternalLink class="size-3" /> Open live view ({ev.ip})
              </a>
            {:else if ev.type === "print"}
              <pre class={`mr-auto max-w-[85%] overflow-x-auto rounded-lg border border-border bg-card px-3 py-2 text-xs ${printToneClass(ev.level)}`}>{ev.message}</pre>
            {:else if ev.type === "devices_changed"}
              <p class="mr-auto text-xs text-muted-foreground">Camera list updated.</p>
            {:else if ev.type === "alerts_changed"}
              <p class="mr-auto text-xs text-muted-foreground">Alerts updated.</p>
            {/if}
          {/each}
        {/if}
      </div>
    {/each}

    {#if busy}
      <div class="mr-auto flex items-center gap-2 text-xs text-muted-foreground">
        <LoaderCircle class="size-3.5 animate-spin" /> Thinking…
      </div>
    {/if}
  </div>

  <div class="flex items-end gap-2 pt-3">
    <Button
      variant={recording ? "destructive" : "outline"}
      size="icon"
      class="shrink-0"
      title={recording ? "Stop recording" : "Speak"}
      onclick={toggleMic}
    >
      {#if recording}<Square class="size-4" />{:else}<Mic class="size-4" />{/if}
    </Button>
    <textarea
      bind:value={input}
      onkeydown={onKeydown}
      rows="1"
      placeholder="Message the assistant…"
      class="flex max-h-40 min-h-10 flex-1 resize-none rounded-lg border border-input bg-background px-3 py-2 text-sm shadow-sm placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
    ></textarea>
    <Button class="shrink-0" size="icon" disabled={!canSend()} onclick={send} title="Send">
      <SendIcon class="size-4" />
    </Button>
  </div>
</div>
