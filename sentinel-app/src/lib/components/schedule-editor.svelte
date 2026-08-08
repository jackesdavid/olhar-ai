<script lang="ts">
  // The calendar-schedule controls, shared by the graph trigger (#111) and the cron-mode
  // `When Event` node. Extracted rather than copied: it is ~60 lines of fiddly per-frequency
  // branching, and two copies would drift the moment one gains a field.
  //
  // Owns no state. The caller holds the schedule and decides how a change is persisted — the graph
  // trigger has its own endpoint and auto-saves, while a node's config is part of the graph and
  // rides the explicit Save. Putting persistence in here would force one of those to be wrong.
  import * as Select from "$lib/components/ui/select";
  import { Input } from "$lib/components/ui/input";

  export type ScheduleFreq = "hourly" | "daily" | "weekly" | "monthly" | "yearly";
  export type ScheduleT = {
    freq: ScheduleFreq;
    tz: string;
    times: string[];
    minute: number;
    weekdays: number[];
    day_of_month: number;
    month: number;
  };

  interface Props {
    value: ScheduleT;
    disabled?: boolean;
    /** Called with the WHOLE next schedule; the caller persists however it must. */
    onchange: (next: ScheduleT) => void;
    labels: { times: string; addTime: string; removeTime: string; timezone: string };
  }
  const { value, disabled = false, onchange, labels }: Props = $props();

  const WEEKDAY_LABELS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
  const MONTH_LABELS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
  const FREQ_OPTS: { value: ScheduleFreq; label: string }[] = [
    { value: "hourly", label: "Every hour" },
    { value: "daily", label: "Daily" },
    { value: "weekly", label: "Weekly" },
    { value: "monthly", label: "Monthly" },
    { value: "yearly", label: "Yearly" },
  ];

  function patch(p: Partial<ScheduleT>) {
    onchange({ ...value, ...p });
  }
  function toggleWeekday(i: number) {
    const has = value.weekdays.includes(i);
    patch({ weekdays: has ? value.weekdays.filter((d) => d !== i) : [...value.weekdays, i].sort((a, b) => a - b) });
  }
  function setTime(i: number, v: string) {
    patch({ times: value.times.map((t, j) => (j === i ? v : t)) });
  }
  function addTime() {
    patch({ times: [...value.times, "09:00"] });
  }
  function removeTime(i: number) {
    patch({ times: value.times.filter((_, j) => j !== i) });
  }
  function freqLabel(f: ScheduleFreq): string {
    return FREQ_OPTS.find((o) => o.value === f)?.label ?? f;
  }
</script>

<div class="space-y-2">
  <Select.Root type="single" {disabled} value={value.freq} onValueChange={(v) => patch({ freq: (v ?? "daily") as ScheduleFreq })}>
    <Select.Trigger class="h-8 w-full">{freqLabel(value.freq)}</Select.Trigger>
    <Select.Content>
      {#each FREQ_OPTS as o (o.value)}<Select.Item value={o.value} label={o.label}>{o.label}</Select.Item>{/each}
    </Select.Content>
  </Select.Root>

  {#if value.freq === "hourly"}
    <div class="flex items-center gap-2">
      <span class="text-xs text-muted-foreground">at minute</span>
      <Input type="number" min="0" max="59" {disabled} value={value.minute} class="h-8 w-20"
        onchange={(e) => patch({ minute: Math.max(0, Math.min(59, Number(e.currentTarget.value))) })} />
    </div>
  {:else}
    {#if value.freq === "weekly"}
      <div class="flex flex-wrap gap-1">
        {#each WEEKDAY_LABELS as wd, i}
          <button type="button" {disabled}
            class="interactive rounded px-2 py-1 text-[11px] {value.weekdays.includes(i) ? 'bg-primary text-primary-foreground' : 'bg-muted text-muted-foreground'}"
            onclick={() => toggleWeekday(i)}>{wd}</button>
        {/each}
      </div>
    {/if}
    {#if value.freq === "monthly"}
      <div class="flex items-center gap-2">
        <span class="text-xs text-muted-foreground">day</span>
        <Input type="number" min="1" max="31" {disabled} value={value.day_of_month} class="h-8 w-20"
          onchange={(e) => patch({ day_of_month: Math.max(1, Math.min(31, Number(e.currentTarget.value))) })} />
      </div>
    {/if}
    {#if value.freq === "yearly"}
      <div class="flex items-center gap-2">
        <Select.Root type="single" {disabled} value={String(value.month)} onValueChange={(v) => patch({ month: Number(v ?? 1) })}>
          <Select.Trigger class="h-8 w-24">{MONTH_LABELS[(value.month || 1) - 1]}</Select.Trigger>
          <Select.Content>
            {#each MONTH_LABELS as m, i}<Select.Item value={String(i + 1)} label={m}>{m}</Select.Item>{/each}
          </Select.Content>
        </Select.Root>
        <Input type="number" min="1" max="31" {disabled} value={value.day_of_month} class="h-8 w-20"
          onchange={(e) => patch({ day_of_month: Math.max(1, Math.min(31, Number(e.currentTarget.value))) })} />
      </div>
    {/if}
    <div class="space-y-1">
      <span class="text-[11px] text-muted-foreground">{labels.times}</span>
      {#each value.times as tm, i (i)}
        <div class="flex items-center gap-1">
          <Input type="time" {disabled} value={tm} class="h-8" onchange={(e) => setTime(i, e.currentTarget.value)} />
          {#if value.times.length > 1}
            <button type="button" {disabled} class="interactive px-1 text-xs text-muted-foreground hover:text-destructive"
              onclick={() => removeTime(i)} aria-label={labels.removeTime}>✕</button>
          {/if}
        </div>
      {/each}
      <button type="button" {disabled} class="interactive text-[11px] text-muted-foreground hover:text-foreground" onclick={addTime}>{labels.addTime}</button>
    </div>
  {/if}

  <div class="space-y-1">
    <span class="text-[11px] text-muted-foreground">{labels.timezone}</span>
    <Input {disabled} value={value.tz} placeholder="America/Sao_Paulo" class="h-8"
      onchange={(e) => patch({ tz: e.currentTarget.value.trim() })} />
  </div>
</div>
