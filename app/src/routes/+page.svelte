<script lang="ts">
  import * as Card from "$lib/components/ui/card";
  import { Badge } from "$lib/components/ui/badge";
  import Cpu from "@lucide/svelte/icons/cpu";
  import Bell from "@lucide/svelte/icons/bell";
  import Video from "@lucide/svelte/icons/video";

  const stats = [
    {
      title: "Devices",
      icon: Cpu,
      value: "—",
      caption: "Connected sensors and cameras",
    },
    {
      title: "Alerts",
      icon: Bell,
      value: "—",
      caption: "Open events requiring attention",
    },
    {
      title: "Live",
      icon: Video,
      value: "—",
      caption: "Active video and audio streams",
    },
  ];
</script>

<div class="space-y-6">
  <div class="flex items-center justify-between">
    <div>
      <h1 class="text-2xl font-semibold tracking-tight">Dashboard</h1>
      <p class="text-sm text-muted-foreground">
        Overview of your surveillance console.
      </p>
    </div>
    <Badge variant="secondary">M0 preview</Badge>
  </div>

  <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
    {#each stats as stat (stat.title)}
      {@const Icon = stat.icon}
      <Card.Root>
        <Card.Header>
          <Card.Title class="flex items-center gap-2 text-sm font-medium">
            <Icon class="size-4 text-muted-foreground" />
            {stat.title}
          </Card.Title>
        </Card.Header>
        <Card.Content>
          <div class="text-3xl font-semibold tracking-tight">{stat.value}</div>
          <p class="mt-1 text-xs text-muted-foreground">{stat.caption}</p>
        </Card.Content>
      </Card.Root>
    {/each}
  </div>

  <Card.Root>
    <Card.Header>
      <Card.Title>Welcome to Sentinel</Card.Title>
      <Card.Description>
        This is the M0 frontend foundation. Feature screens are being migrated
        from the legacy terminal application and will light up as the backend
        endpoints are wired in.
      </Card.Description>
    </Card.Header>
  </Card.Root>
</div>
