<script lang="ts">
  // Voice-first assistant. The Sentinel dot-field orb sits center stage and reacts to
  // live audio — emerald while Sentinel speaks, sky-blue while the operator speaks.
  // Spoken replies reveal their text in time with the audio; dictation is sent straight
  // to the chat; hold Ctrl or the orb to push-to-talk.
  import { onDestroy, tick } from "svelte";
  import { toast } from "svelte-sonner";

  import SendIcon from "@lucide/svelte/icons/send";
  import Mic from "@lucide/svelte/icons/mic";
  import Busy from "$lib/components/busy.svelte";
  import Volume2 from "@lucide/svelte/icons/volume-2";
  import VolumeX from "@lucide/svelte/icons/volume-x";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import Camera from "@lucide/svelte/icons/camera";

  import { Button } from "$lib/components/ui/button";
  import VoiceOrb from "$lib/components/voice-orb.svelte";
  import { agent, type ChatTurn, type ChatToolEvent } from "$lib/api";
  import { dict, lang } from "$lib/i18n";

  const t = $derived(dict[lang()].chat);

  // `reveal` = number of words currently shown (audio-synced); undefined = show all.
  type Msg = { role: "user" | "assistant"; content: string; tools?: ChatToolEvent[]; reveal?: number };

  let messages = $state<Msg[]>([]);
  let input = $state("");
  let busy = $state(false);
  let recording = $state(false);
  let speakReplies = $state(true);
  let voiceState = $state<"idle" | "listening" | "speaking">("idle");
  let level = $state(0);

  let scroller = $state<HTMLDivElement | null>(null);

  // Audio plumbing (not reactive state).
  let actx: AudioContext | null = null;
  let analyser: AnalyserNode | null = null;
  let rafId = 0;
  let player: HTMLAudioElement | null = null;
  let recorder: MediaRecorder | null = null;
  let chunks: Blob[] = [];
  let micStream: MediaStream | null = null;

  // ---- amplitude meter (drives the orb) -------------------------------------
  function meterLoop() {
    rafId = requestAnimationFrame(meterLoop);
    if (!analyser) {
      if (level !== 0) level = 0;
      return;
    }
    const buf = new Uint8Array(analyser.fftSize);
    analyser.getByteTimeDomainData(buf);
    let sum = 0;
    for (const v of buf) {
      const x = (v - 128) / 128;
      sum += x * x;
    }
    level = Math.min(1, Math.sqrt(sum / buf.length) * 3.2);
  }
  if (typeof window !== "undefined") rafId = requestAnimationFrame(meterLoop);

  async function ensureCtx(): Promise<AudioContext> {
    if (!actx) actx = new AudioContext();
    if (actx.state === "suspended") await actx.resume();
    return actx;
  }

  function statusLine(): string {
    if (recording) return t.listening;
    if (voiceState === "speaking") return t.speaking;
    if (busy) return t.thinking;
    return t.holdToTalkHint;
  }

  function bubbleClass(role: Msg["role"]): string {
    return role === "user"
      ? "ml-auto bg-primary text-primary-foreground"
      : "mr-auto border border-border bg-card text-card-foreground";
  }

  function printToneClass(lvl: string): string {
    if (lvl === "error") return "text-destructive";
    if (lvl === "warn") return "text-amber-500";
    return "text-muted-foreground";
  }

  // Text shown for a message — sliced to the audio-synced reveal count when speaking.
  function shownText(m: Msg): string {
    if (m.reveal == null) return m.content;
    return m.content.split(/\s+/).slice(0, m.reveal).join(" ");
  }

  function canSend(): boolean {
    return !busy && input.trim().length > 0;
  }

  async function scrollToEnd() {
    await tick();
    scroller?.scrollTo({ top: scroller.scrollHeight, behavior: "smooth" });
  }

  // ---- send a turn ----------------------------------------------------------
  async function send(text?: string) {
    const msg = (text ?? input).trim();
    if (!msg || busy) return;
    input = "";
    messages.push({ role: "user", content: msg });
    void scrollToEnd();

    const history: ChatTurn[] = messages.map((m) => ({ role: m.role, content: m.content }));
    busy = true;
    try {
      const reply = await agent.chat(history);
      if (speakReplies && reply.text.trim()) {
        await speakAndReveal(reply.text, reply.tool_events);
      } else {
        messages.push({ role: "assistant", content: reply.text, tools: reply.tool_events });
        void scrollToEnd();
      }
    } catch (e) {
      toast.error(e instanceof Error ? e.message : t.noReply);
    } finally {
      busy = false;
    }
  }

  // Speak the reply and reveal its text word-by-word in time with the audio. Falls back
  // to showing the text immediately if voice is unavailable or autoplay is blocked.
  async function speakAndReveal(text: string, tools: ChatToolEvent[]) {
    let blob: Blob | null = null;
    try {
      blob = await agent.tts(text);
    } catch {
      blob = null;
    }
    if (!blob) {
      messages.push({ role: "assistant", content: text, tools });
      void scrollToEnd();
      return;
    }

    const idx = messages.push({ role: "assistant", content: text, tools, reveal: 0 }) - 1;
    void scrollToEnd();
    const words = text.split(/\s+/).length;
    const url = URL.createObjectURL(blob);
    const ctx = await ensureCtx();
    const audio = new Audio(url);
    player?.pause();
    player = audio;

    const srcNode = ctx.createMediaElementSource(audio);
    const an = ctx.createAnalyser();
    an.fftSize = 256;
    srcNode.connect(an);
    an.connect(ctx.destination);

    const finish = () => {
      messages[idx].reveal = undefined; // ensure full text is shown
      if (player === audio) {
        voiceState = "idle";
        analyser = null;
        level = 0;
      }
      URL.revokeObjectURL(url);
    };

    audio.onplay = () => {
      voiceState = "speaking";
      analyser = an;
    };
    audio.ontimeupdate = () => {
      if (audio.duration > 0) {
        messages[idx].reveal = Math.min(words, Math.ceil((words * audio.currentTime) / audio.duration));
        void scrollToEnd();
      }
    };
    audio.onended = finish;
    audio.onerror = finish;

    try {
      await audio.play();
    } catch {
      finish(); // autoplay blocked — reveal the whole reply
    }
  }

  // ---- push-to-talk ---------------------------------------------------------
  async function startRec() {
    if (recording || busy) return;
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      micStream = stream;
      const ctx = await ensureCtx();
      const src = ctx.createMediaStreamSource(stream);
      const an = ctx.createAnalyser();
      an.fftSize = 256;
      src.connect(an); // not to destination — no mic echo
      analyser = an;
      voiceState = "listening";

      chunks = [];
      recorder = new MediaRecorder(stream);
      recorder.ondataavailable = (e) => e.data.size > 0 && chunks.push(e.data);
      recorder.onstop = () => {
        stream.getTracks().forEach((t) => t.stop());
        micStream = null;
        recording = false;
        analyser = null;
        level = 0;
        voiceState = "idle";
        const discard = pttDiscard;
        pttDiscard = false;
        if (!discard) void transcribeAndSend();
      };
      recorder.start();
      recording = true;
    } catch {
      voiceState = "idle";
      toast.error(t.micFailed);
    }
  }

  // `discard` drops the clip without transcribing (used when Ctrl turns out to be a shortcut).
  let pttDiscard = false;
  function stopRec(discard = false) {
    if (recording) {
      pttDiscard = discard;
      recorder?.stop();
    }
  }

  // Push-to-talk intent, shared by the key + button. Tracks "held" so a release that
  // lands during the async getUserMedia gap still stops the recording once it starts.
  let pttHeld = false;
  async function pttStart() {
    if (pttHeld || busy) return; // dedupe key-repeat / ignore mid-turn
    pttHeld = true;
    await startRec();
    if (!pttHeld) stopRec(); // released before the mic finished opening
  }
  function pttStop() {
    pttHeld = false;
    stopRec(false);
  }
  function pttCancel() {
    pttHeld = false;
    stopRec(true);
  }

  // Transcribe the clip and send it straight to the chat (no manual edit step).
  async function transcribeAndSend() {
    if (chunks.length === 0) return;
    const blob = new Blob(chunks, { type: recorder?.mimeType || "audio/webm" });
    if (blob.size < 1200) return; // ignore an accidental tap — too short to transcribe
    busy = true;
    let text = "";
    try {
      text = await agent.stt(blob);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : t.transcriptionFailed);
      busy = false;
      return;
    }
    busy = false;
    if (text.trim()) await send(text);
  }

  // ---- key handling ---------------------------------------------------------
  // Push-to-talk: hold Ctrl. A modifier never inserts text, so it works even while the
  // message box is focused. If another key is pressed while Ctrl is down, it's a shortcut
  // (Ctrl+C, …) — abort the recording without sending.
  let ctrlPtt = false;
  function onWindowKeydown(e: KeyboardEvent) {
    if (e.key === "Control") {
      if (e.repeat || ctrlPtt) return;
      ctrlPtt = true;
      void pttStart();
      return;
    }
    if (ctrlPtt) {
      ctrlPtt = false;
      pttCancel(); // Ctrl + something = a shortcut, not speech
    }
  }
  function onWindowKeyup(e: KeyboardEvent) {
    if (e.key === "Control" && ctrlPtt) {
      ctrlPtt = false;
      pttStop();
    }
  }
  function onInputKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void send();
    }
  }

  onDestroy(() => {
    cancelAnimationFrame(rafId);
    player?.pause();
    micStream?.getTracks().forEach((t) => t.stop());
    void actx?.close();
  });
</script>

<svelte:window onkeydown={onWindowKeydown} onkeyup={onWindowKeyup} />

<div class="mx-auto flex h-[calc(100dvh-7rem)] max-w-3xl flex-col">
  <div class="flex items-start justify-between gap-3 pb-2">
    <div>
      <h1 class="text-2xl font-semibold tracking-tight">{t.title}</h1>
      <p class="text-sm text-muted-foreground">{t.description}</p>
    </div>
    <Button
      variant="ghost"
      size="icon"
      title={speakReplies ? t.muteReplies : t.speakReplies}
      onclick={() => (speakReplies = !speakReplies)}
    >
      {#if speakReplies}<Volume2 class="size-4" />{:else}<VolumeX class="size-4" />{/if}
    </Button>
  </div>

  <!-- center stage: the audio-reactive orb doubles as the push-to-talk control -->
  <div class="flex flex-col items-center justify-center gap-3 py-4">
    <button
      type="button"
      class="interactive rounded-full transition-transform focus-visible:outline-none active:scale-95 disabled:opacity-50"
      style="touch-action:none"
      title={t.holdToTalk}
      aria-label={t.holdToTalkLabel}
      disabled={busy}
      onpointerdown={(e) => {
        e.preventDefault();
        void pttStart();
      }}
      onpointerup={pttStop}
      onpointerleave={pttStop}
      onpointercancel={pttStop}
    >
      <VoiceOrb {level} state={voiceState} size={220} />
    </button>
    <p class="text-sm font-medium text-muted-foreground">{statusLine()}</p>
  </div>

  <!-- transcript -->
  <div bind:this={scroller} class="flex-1 space-y-4 overflow-y-auto rounded-xl border border-border bg-elev-1 p-4">
    {#if messages.length === 0}
      <div class="flex h-full flex-col items-center justify-center gap-2 text-center text-muted-foreground">
        <p class="max-w-sm text-sm">{t.emptyHint}</p>
      </div>
    {/if}

    {#each messages as m (m)}
      <div class="flex flex-col gap-2">
        <div class={`max-w-[85%] whitespace-pre-wrap rounded-2xl px-3.5 py-2 text-sm leading-relaxed ${bubbleClass(m.role)}`}>
          {shownText(m)}
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
              <a href="/live" class="interactive mr-auto inline-flex items-center gap-1.5 rounded-lg border border-border bg-card px-3 py-1.5 text-xs font-medium text-primary">
                <ExternalLink class="size-3" /> {t.openLive(ev.ip)}
              </a>
            {:else if ev.type === "print"}
              <pre class={`mr-auto max-w-[85%] overflow-x-auto rounded-lg border border-border bg-card px-3 py-2 text-xs ${printToneClass(ev.level)}`}>{ev.message}</pre>
            {:else if ev.type === "devices_changed"}
              <p class="mr-auto text-xs text-muted-foreground">{t.devicesChanged}</p>
            {:else if ev.type === "alerts_changed"}
              <p class="mr-auto text-xs text-muted-foreground">{t.alertsChanged}</p>
            {/if}
          {/each}
        {/if}
      </div>
    {/each}

    {#if busy}
      <div class="mr-auto flex items-center gap-2 text-xs text-muted-foreground">
        <Busy size={14} /> {t.thinking}
      </div>
    {/if}
  </div>

  <!-- input row -->
  <div class="flex items-end gap-2 pt-3">
    <Button
      variant={recording ? "destructive" : "outline"}
      size="icon"
      class="shrink-0"
      title={t.holdToTalkSpace}
      onpointerdown={(e) => {
        e.preventDefault();
        void pttStart();
      }}
      onpointerup={pttStop}
      onpointerleave={pttStop}
    >
      <Mic class="size-4" />
    </Button>
    <textarea
      bind:value={input}
      onkeydown={onInputKeydown}
      rows="1"
      placeholder={t.placeholder}
      class="flex max-h-40 min-h-10 flex-1 resize-none rounded-lg border border-input bg-background px-3 py-2 text-sm shadow-sm placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
    ></textarea>
    <Button class="shrink-0" size="icon" disabled={!canSend()} onclick={() => send()} title={t.send}>
      <SendIcon class="size-4" />
    </Button>
  </div>
</div>
