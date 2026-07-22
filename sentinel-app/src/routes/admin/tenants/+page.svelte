<script lang="ts">
  import PageHeader from "$lib/components/page-header.svelte";
  import { goto } from "$app/navigation";
  import { toast } from "svelte-sonner";

  import Busy from "$lib/components/busy.svelte";
  import Building2 from "@lucide/svelte/icons/building-2";
  import Plus from "@lucide/svelte/icons/plus";
  import LogIn from "@lucide/svelte/icons/log-in";
  import EmptyState from "$lib/components/empty-state.svelte";

  import * as Card from "$lib/components/ui/card";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Badge } from "$lib/components/ui/badge";
  import Loader from "$lib/components/loader.svelte";

  import { admin, type Tenant } from "$lib/api";
  import { auth, assumeTenant } from "$lib/auth.svelte";

  let loading = $state(true);
  let tenants = $state<Tenant[]>([]);
  let assuming = $state<string | null>(null);

  let createOpen = $state(false);
  let tName = $state("");
  let tEmail = $state("");
  let tPassword = $state("");
  let creating = $state(false);

  const isMaster = auth.me?.is_master ?? false;

  async function load() {
    loading = true;
    try {
      tenants = await admin.tenants();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Failed to load tenants");
    } finally {
      loading = false;
    }
  }
  $effect(() => {
    if (isMaster) load();
    else loading = false;
  });

  async function navigateAs(t: Tenant) {
    assuming = t.id;
    try {
      await assumeTenant(t.id);
      toast.success(`Now acting as ${t.name}`);
      await goto("/");
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Could not navigate as tenant");
    } finally {
      assuming = null;
    }
  }

  async function createTenant(e: SubmitEvent) {
    e.preventDefault();
    if (creating) return;
    creating = true;
    try {
      await admin.createTenant(tName.trim(), tEmail.trim(), tPassword);
      createOpen = false;
      tName = "";
      tEmail = "";
      tPassword = "";
      await load();
      toast.success("Tenant created");
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Failed to create tenant");
    } finally {
      creating = false;
    }
  }

  const canCreate = $derived(tName.trim() && tEmail.trim().includes("@") && tPassword.length >= 8);
</script>

<div class="space-y-6">
  <PageHeader
    title="Tenants"
    description="Every workspace on this Sentinel instance."
    icon={Building2}
  >
    {#snippet actions()}
      {#if isMaster}
        <Button onclick={() => (createOpen = true)}><Plus class="size-4" /> New tenant</Button>
      {/if}
    {/snippet}
  </PageHeader>

  {#if !isMaster}
    <Card.Root><Card.Content class="py-10 text-center text-sm text-muted-foreground">
      This console is for the SaaS administrator only.
    </Card.Content></Card.Root>
  {:else if loading}
    <Loader />
  {:else}
    <Card.Root>
      <Card.Content class="overflow-x-auto pt-6">
        <table class="w-full border-collapse text-sm">
          <thead>
            <tr class="border-b border-border">
              <th class="py-2 pr-4 text-left font-medium">Name</th>
              <th class="px-3 py-2 text-left font-medium">Slug</th>
              <th class="px-3 py-2 text-left font-medium">Status</th>
              <th class="px-3 py-2 text-right font-medium"></th>
            </tr>
          </thead>
          <tbody>
            {#each tenants as t (t.id)}
              <tr class="border-b border-border/50 last:border-0">
                <td class="py-2 pr-4 font-medium">{t.name}</td>
                <td class="px-3 py-2 font-mono text-xs text-muted-foreground">{t.slug}</td>
                <td class="px-3 py-2"><Badge variant="secondary" class="text-[10px]">{t.status}</Badge></td>
                <td class="px-3 py-2 text-right">
                  <Button variant="outline" size="sm" disabled={assuming === t.id} onclick={() => navigateAs(t)}>
                    {#if assuming === t.id}<Busy size={14} />{:else}<LogIn class="size-3.5" />{/if}
                    Navigate as tenant
                  </Button>
                </td>
              </tr>
            {/each}
            {#if tenants.length === 0}
              <tr><td colspan={4}>
                <EmptyState
                  title="No tenants yet"
                  description="Create the first tenant to onboard a customer workspace."
                />
              </td></tr>
            {/if}
          </tbody>
        </table>
      </Card.Content>
    </Card.Root>
    <p class="text-xs text-muted-foreground">
      Navigating as a tenant lets you manage its users, roles, and data — but live camera feeds stay private and are
      never accessible, even to you.
    </p>
  {/if}
</div>

<Dialog.Root bind:open={createOpen}>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>New tenant</Dialog.Title>
      <Dialog.Description>Creates a workspace and its first admin user.</Dialog.Description>
    </Dialog.Header>
    <form class="space-y-4" onsubmit={createTenant}>
      <div class="space-y-1.5">
        <Label for="t-name">Workspace name</Label>
        <Input id="t-name" bind:value={tName} placeholder="Acme Security" />
      </div>
      <div class="space-y-1.5">
        <Label for="t-email">Admin email</Label>
        <Input id="t-email" type="email" bind:value={tEmail} placeholder="admin@acme.com" />
      </div>
      <div class="space-y-1.5">
        <Label for="t-pw">Admin password</Label>
        <Input id="t-pw" type="password" bind:value={tPassword} placeholder="At least 8 characters" autocomplete="new-password" />
      </div>
      <Dialog.Footer>
        <Button type="submit" disabled={!canCreate || creating}>
          {#if creating}<Busy />{/if}
          Create tenant
        </Button>
      </Dialog.Footer>
    </form>
  </Dialog.Content>
</Dialog.Root>
