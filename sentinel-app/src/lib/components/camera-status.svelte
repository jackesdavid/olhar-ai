<script lang="ts">
  // Camera status (#155) — WHICH stage a camera is in, in the user's words, with the action that
  // follows from it.
  //
  // The bar this has to clear: before it existed, an unplugged camera showed `has_stream: true`
  // and the frame endpoint answered 502 "the camera's decoder produced nothing". Diagnosing it
  // took `ping` and `nc` on the edge box. A colour alone would not have helped — the useful part
  // is naming the stage and what to do about it.
  import { Badge } from "$lib/components/ui/badge";
  import { dict, lang } from "$lib/i18n";
  import type { CameraStatus } from "$lib/types";

  let {
    status = null,
    detail = null,
    checkedAt = null,
    compact = false,
  }: {
    status?: CameraStatus | null;
    detail?: string | null;
    checkedAt?: string | null;
    compact?: boolean;
  } = $props();

  const t = $derived(dict[lang()].devices);
  const key = $derived(status ?? "unknown");
  const label = $derived(t.status[key as keyof typeof t.status] ?? t.status.unknown);
  const hint = $derived(t.statusHint[key as keyof typeof t.statusHint] ?? "");

  // Named helper rather than logic in the template (house style). Only `streaming` is genuinely
  // healthy: `ok` means the stream could be DESCRIBED, which a wedged camera also manages.
  function tone(s: string): "default" | "secondary" | "destructive" | "outline" {
    if (s === "streaming") return "default";
    if (s === "ok") return "secondary";
    if (s === "unknown" || s === "not_on_this_box") return "outline";
    return "destructive";
  }

  /** "checked 40s ago" — a status with no age is one nobody can trust. */
  function age(iso: string | null): string {
    if (!iso) return t.never;
    const secs = Math.max(0, Math.floor((Date.now() - new Date(iso).getTime()) / 1000));
    if (secs < 60) return `${secs}s`;
    if (secs < 3600) return `${Math.floor(secs / 60)}m`;
    if (secs < 86400) return `${Math.floor(secs / 3600)}h`;
    return `${Math.floor(secs / 86400)}d`;
  }
</script>

<div class="flex flex-col gap-1">
  <div class="flex items-center gap-2">
    <Badge variant={tone(key)} class="text-[10px]">{label}</Badge>
    {#if checkedAt}
      <span class="text-[10px] text-muted-foreground">{t.checkedAt} {age(checkedAt)}</span>
    {/if}
  </div>
  {#if !compact && hint}
    <span class="text-xs text-muted-foreground">{hint}</span>
  {/if}
  {#if !compact && detail}
    <!-- The camera's OWN words. The status says which stage; this says why within it. -->
    <span class="font-mono text-[10px] text-muted-foreground/80">{detail}</span>
  {/if}
</div>
