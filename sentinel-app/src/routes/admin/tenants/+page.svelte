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
  import { dict, lang } from "$lib/i18n";

  const t = $derived(dict[lang()].admin);

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
      toast.error(e instanceof Error ? e.message : t.tenantsLoadFailed);
    } finally {
      loading = false;
    }
  }
  $effect(() => {
    if (isMaster) load();
    else loading = false;
  });

  async function navigateAs(tn: Tenant) {
    assuming = tn.id;
    try {
      await assumeTenant(tn.id);
      toast.success(t.nowActingAs(tn.name));
      await goto("/");
    } catch (e) {
      toast.error(e instanceof Error ? e.message : t.navigateFailed);
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
      toast.success(t.tenantCreated);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : t.tenantCreateFailed);
    } finally {
      creating = false;
    }
  }

  const canCreate = $derived(tName.trim() && tEmail.trim().includes("@") && tPassword.length >= 8);
</script>

<div class="space-y-6">
  <PageHeader
    title={t.tenantsTitle}
    description={t.tenantsDescription}
    icon={Building2}
  >
    {#snippet actions()}
      {#if isMaster}
        <Button onclick={() => (createOpen = true)}><Plus class="size-4" /> {t.newTenant}</Button>
      {/if}
    {/snippet}
  </PageHeader>

  {#if !isMaster}
    <Card.Root><Card.Content class="py-10 text-center text-sm text-muted-foreground">
      {t.consoleOnly}
    </Card.Content></Card.Root>
  {:else if loading}
    <Loader />
  {:else}
    <Card.Root>
      <Card.Content class="overflow-x-auto pt-6">
        <table class="w-full border-collapse text-sm">
          <thead>
            <tr class="border-b border-border">
              <th class="py-2 pr-4 text-left font-medium">{t.colName}</th>
              <th class="px-3 py-2 text-left font-medium">{t.colSlug}</th>
              <th class="px-3 py-2 text-left font-medium">{t.colStatus}</th>
              <th class="px-3 py-2 text-right font-medium"></th>
            </tr>
          </thead>
          <tbody>
            {#each tenants as tn (tn.id)}
              <tr class="border-b border-border/50 last:border-0">
                <td class="py-2 pr-4 font-medium">{tn.name}</td>
                <td class="px-3 py-2 font-mono text-xs text-muted-foreground">{tn.slug}</td>
                <td class="px-3 py-2"><Badge variant="secondary" class="text-[10px]">{tn.status}</Badge></td>
                <td class="px-3 py-2 text-right">
                  <Button variant="outline" size="sm" disabled={assuming === tn.id} onclick={() => navigateAs(tn)}>
                    {#if assuming === tn.id}<Busy size={14} />{:else}<LogIn class="size-3.5" />{/if}
                    {t.navigateAsTenant}
                  </Button>
                </td>
              </tr>
            {/each}
            {#if tenants.length === 0}
              <tr><td colspan={4}>
                <EmptyState
                  title={t.tenantsEmptyTitle}
                  description={t.tenantsEmptyDescription}
                />
              </td></tr>
            {/if}
          </tbody>
        </table>
      </Card.Content>
    </Card.Root>
    <p class="text-xs text-muted-foreground">{t.navigateNote}</p>
  {/if}
</div>

<Dialog.Root bind:open={createOpen}>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>{t.newTenant}</Dialog.Title>
      <Dialog.Description>{t.newTenantDescription}</Dialog.Description>
    </Dialog.Header>
    <form class="space-y-4" onsubmit={createTenant}>
      <div class="space-y-1.5">
        <Label for="t-name">{t.workspaceName}</Label>
        <Input id="t-name" bind:value={tName} placeholder={t.workspaceNamePlaceholder} />
      </div>
      <div class="space-y-1.5">
        <Label for="t-email">{t.adminEmail}</Label>
        <Input id="t-email" type="email" bind:value={tEmail} placeholder={t.adminEmailPlaceholder} />
      </div>
      <div class="space-y-1.5">
        <Label for="t-pw">{t.adminPassword}</Label>
        <Input id="t-pw" type="password" bind:value={tPassword} placeholder={t.adminPasswordPlaceholder} autocomplete="new-password" />
      </div>
      <Dialog.Footer>
        <Button type="submit" disabled={!canCreate || creating}>
          {#if creating}<Busy />{/if}
          {t.createTenant}
        </Button>
      </Dialog.Footer>
    </form>
  </Dialog.Content>
</Dialog.Root>
