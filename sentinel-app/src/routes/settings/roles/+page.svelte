<script lang="ts">
  import PageHeader from "$lib/components/page-header.svelte";
  import { toast } from "svelte-sonner";

  import Busy from "$lib/components/busy.svelte";
  import Loader from "$lib/components/loader.svelte";
  import EmptyState from "$lib/components/empty-state.svelte";
  import Plus from "@lucide/svelte/icons/plus";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";

  import * as Card from "$lib/components/ui/card";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Badge } from "$lib/components/ui/badge";

  import { rbac, type Permission, type Role } from "$lib/api";
  import { hasPerm } from "$lib/auth.svelte";
  import { dict, lang } from "$lib/i18n";

  const t = $derived(dict[lang()].roles);
  const c = $derived(dict[lang()].common);

  /** Removing this from a role can lock the caller out of this very screen. */
  const SELF_LOCKOUT_KEY = "roles.manage";

  let loading = $state(true);
  let permissions = $state<Permission[]>([]);
  let roles = $state<Role[]>([]);
  /** roleId → granted permission keys. */
  let matrix = $state<Record<string, Set<string>>>({});
  let saving = $state<string | null>(null);

  let createOpen = $state(false);
  let newRole = $state("");
  let creating = $state(false);

  /** Pending toggle held back by the lockout confirmation. */
  let confirming = $state<{ role: Role; key: string } | null>(null);

  // Reactive: `auth.me` may resolve after this component initialises.
  const allowed = $derived(hasPerm("roles.manage"));

  async function load() {
    loading = true;
    try {
      [permissions, roles] = await Promise.all([rbac.permissions(), rbac.roles()]);
      const entries = await Promise.all(
        roles.map(async (r) => [r.id, new Set(await rbac.rolePerms(r.id))] as const),
      );
      matrix = Object.fromEntries(entries);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : t.loadFailed);
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    if (allowed) load();
    else loading = false;
  });

  // ---- permission grouping (one section per resource: devices, alerts, billing…) ----

  function resourceOf(key: string): string {
    return key.split(".")[0] ?? key;
  }

  function groupLabel(resource: string): string {
    return resource.charAt(0).toUpperCase() + resource.slice(1);
  }

  /** `[resource, permissions][]` preserving the catalog's order. */
  const groups = $derived.by(() => {
    const byResource = new Map<string, Permission[]>();
    for (const p of permissions) {
      const r = resourceOf(p.key);
      if (!byResource.has(r)) byResource.set(r, []);
      byResource.get(r)!.push(p);
    }
    return [...byResource.entries()];
  });

  function actionOf(key: string): string {
    return key.slice(resourceOf(key).length + 1) || key;
  }

  // ---- matrix ----

  function isGranted(roleId: string, key: string): boolean {
    return matrix[roleId]?.has(key) ?? false;
  }

  function grantedCount(roleId: string): number {
    return matrix[roleId]?.size ?? 0;
  }

  /** The dot IS the control: filled = granted, hollow = withheld. */
  function dotClass(granted: boolean): string {
    if (granted) return "border-primary bg-primary";
    return "border-border bg-transparent hover:border-primary/60";
  }

  function requestToggle(role: Role, key: string) {
    // Turning off `roles.manage` can strip the caller's access to this screen.
    if (key === SELF_LOCKOUT_KEY && isGranted(role.id, key)) {
      confirming = { role, key };
      return;
    }
    void applyToggle(role, key);
  }

  async function applyToggle(role: Role, key: string) {
    const set = new Set(matrix[role.id] ?? []);
    if (set.has(key)) set.delete(key);
    else set.add(key);
    matrix = { ...matrix, [role.id]: set };
    saving = role.id;
    try {
      await rbac.setRolePerms(role.id, [...set]);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : t.saveFailed);
      await load(); // re-sync from the server
    } finally {
      saving = null;
    }
  }

  async function confirmToggle() {
    if (!confirming) return;
    const { role, key } = confirming;
    confirming = null;
    await applyToggle(role, key);
  }

  const canCreate = $derived(newRole.trim().length > 0);

  async function createRole(e: SubmitEvent) {
    e.preventDefault();
    if (!canCreate || creating) return;
    const name = newRole.trim();
    creating = true;
    try {
      await rbac.createRole(name);
      newRole = "";
      createOpen = false;
      await load();
      toast.success(t.created(name));
    } catch (e) {
      toast.error(e instanceof Error ? e.message : t.createFailed);
    } finally {
      creating = false;
    }
  }
</script>

<div class="space-y-6">
  <PageHeader
    title={t.title}
    description={t.description}
    icon={ShieldCheck}
  >
    {#snippet actions()}
      {#if allowed}
        <Button onclick={() => (createOpen = true)}><Plus class="size-4" /> {t.newRole}</Button>
      {/if}
    {/snippet}
  </PageHeader>

  {#if !allowed}
    <Card.Root>
      <Card.Content class="py-4">
        <EmptyState
          title={t.noPermissionTitle}
          description={t.noPermissionDescription}
        />
      </Card.Content>
    </Card.Root>
  {:else if loading}
    <Loader />
  {:else if roles.length === 0}
    <Card.Root>
      <Card.Content class="py-4">
        <EmptyState title={t.emptyTitle} description={t.emptyDescription}>
          <Button onclick={() => (createOpen = true)}><Plus class="size-4" /> {t.newRole}</Button>
        </EmptyState>
      </Card.Content>
    </Card.Root>
  {:else}
    <Card.Root>
      <Card.Content class="overflow-x-auto pt-6">
        <table class="w-full border-collapse text-sm">
          <thead>
            <tr class="border-b border-border">
              <th class="sticky left-0 z-10 bg-card py-2 pr-6 text-left align-bottom">
                <span class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">{t.action}</span>
              </th>
              {#each roles as role (role.id)}
                <th class="px-4 py-2 text-center align-bottom font-medium">
                  <div class="flex flex-col items-center gap-1">
                    <div class="inline-flex items-center gap-1.5">
                      <span>{role.name}</span>
                      {#if role.is_system}
                        <Badge variant="secondary" class="text-[10px]">{t.system}</Badge>
                      {/if}
                      {#if saving === role.id}<Busy size={12} class="text-muted-foreground" />{/if}
                    </div>
                    <span class="text-[10px] tabular-nums text-muted-foreground">
                      {grantedCount(role.id)} / {permissions.length}
                    </span>
                  </div>
                </th>
              {/each}
            </tr>
          </thead>
          <tbody>
            {#each groups as [resource, perms] (resource)}
              <tr>
                <td colspan={roles.length + 1} class="sticky left-0 bg-card pb-1.5 pt-5">
                  <span class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                    {groupLabel(resource)}
                  </span>
                </td>
              </tr>
              {#each perms as perm (perm.key)}
                <tr class="interactive border-b border-border/50">
                  <td class="sticky left-0 bg-card py-2 pr-6">
                    <div class="font-medium">{actionOf(perm.key)}</div>
                    <div class="text-xs text-muted-foreground">{perm.description}</div>
                  </td>
                  {#each roles as role (role.id)}
                    <td class="px-4 py-2 text-center">
                      <button
                        type="button"
                        role="switch"
                        aria-checked={isGranted(role.id, perm.key)}
                        aria-label={`${role.name}: ${perm.key}`}
                        disabled={saving === role.id}
                        onclick={() => requestToggle(role, perm.key)}
                        class="press size-4 rounded-full border transition-colors duration-150 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/40 disabled:opacity-40 {dotClass(
                          isGranted(role.id, perm.key),
                        )}"
                      ></button>
                    </td>
                  {/each}
                </tr>
              {/each}
            {/each}
          </tbody>
        </table>
      </Card.Content>
    </Card.Root>

    <p class="text-xs text-muted-foreground">
      {t.footnoteBefore}<span class="font-medium text-foreground">{t.admin}</span>{t.footnoteAfter}
    </p>
  {/if}
</div>

<!-- New role -->
<Dialog.Root bind:open={createOpen}>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>{t.newRole}</Dialog.Title>
      <Dialog.Description>{t.createDescription}</Dialog.Description>
    </Dialog.Header>
    <form class="space-y-4" onsubmit={createRole}>
      <div class="space-y-1.5">
        <Label for="role-name">{t.roleName}</Label>
        <Input id="role-name" bind:value={newRole} placeholder={t.roleNamePlaceholder} />
      </div>
      <Dialog.Footer>
        <Button type="submit" disabled={!canCreate || creating}>
          {#if creating}<Busy />{/if}
          {t.createRole}
        </Button>
      </Dialog.Footer>
    </form>
  </Dialog.Content>
</Dialog.Root>

<!-- Guard against removing the action that gates this very screen -->
<Dialog.Root open={confirming !== null} onOpenChange={(o) => { if (!o) confirming = null; }}>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>{t.lockoutTitle(SELF_LOCKOUT_KEY)}</Dialog.Title>
      <Dialog.Description>
        {#if confirming}
          {t.lockoutBefore}<span class="font-medium text-foreground">{confirming.role.name}</span>{t.lockoutAfter}
        {/if}
      </Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (confirming = null)}>{c.cancel}</Button>
      <Button variant="destructive" onclick={confirmToggle}>{t.removeAnyway}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
