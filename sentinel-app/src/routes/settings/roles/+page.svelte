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
      toast.error(e instanceof Error ? e.message : "Failed to load roles");
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
      toast.error(e instanceof Error ? e.message : "Failed to save");
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
      toast.success(`Role "${name}" created`);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Failed to create role");
    } finally {
      creating = false;
    }
  }
</script>

<div class="space-y-6">
  <PageHeader
    title="Roles & permissions"
    description="Access is granted by action, never by role name. Changes save as you toggle."
    icon={ShieldCheck}
  >
    {#snippet actions()}
      {#if allowed}
        <Button onclick={() => (createOpen = true)}><Plus class="size-4" /> New role</Button>
      {/if}
    {/snippet}
  </PageHeader>

  {#if !allowed}
    <Card.Root>
      <Card.Content class="py-4">
        <EmptyState
          title="You can't manage roles"
          description="Ask an administrator for the roles.manage action to edit the permission matrix."
        />
      </Card.Content>
    </Card.Root>
  {:else if loading}
    <Loader />
  {:else if roles.length === 0}
    <Card.Root>
      <Card.Content class="py-4">
        <EmptyState title="No roles yet" description="Create a role, then grant it the actions it needs.">
          <Button onclick={() => (createOpen = true)}><Plus class="size-4" /> New role</Button>
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
                <span class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">Action</span>
              </th>
              {#each roles as role (role.id)}
                <th class="px-4 py-2 text-center align-bottom font-medium">
                  <div class="flex flex-col items-center gap-1">
                    <div class="inline-flex items-center gap-1.5">
                      <span>{role.name}</span>
                      {#if role.is_system}
                        <Badge variant="secondary" class="text-[10px]">system</Badge>
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
      A filled dot grants the action to that role. System roles come with sensible defaults —
      <span class="font-medium text-foreground">Admin</span> always receives every action, including new ones.
    </p>
  {/if}
</div>

<!-- New role -->
<Dialog.Root bind:open={createOpen}>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>New role</Dialog.Title>
      <Dialog.Description>Create a custom role, then grant it actions in the matrix.</Dialog.Description>
    </Dialog.Header>
    <form class="space-y-4" onsubmit={createRole}>
      <div class="space-y-1.5">
        <Label for="role-name">Role name</Label>
        <Input id="role-name" bind:value={newRole} placeholder="e.g. Operator" />
      </div>
      <Dialog.Footer>
        <Button type="submit" disabled={!canCreate || creating}>
          {#if creating}<Busy />{/if}
          Create role
        </Button>
      </Dialog.Footer>
    </form>
  </Dialog.Content>
</Dialog.Root>

<!-- Guard against removing the action that gates this very screen -->
<Dialog.Root open={confirming !== null} onOpenChange={(o) => { if (!o) confirming = null; }}>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>Remove “{SELF_LOCKOUT_KEY}”?</Dialog.Title>
      <Dialog.Description>
        {#if confirming}
          If you hold the <span class="font-medium text-foreground">{confirming.role.name}</span> role, you will lose
          access to this screen and won't be able to grant it back yourself.
        {/if}
      </Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (confirming = null)}>Cancel</Button>
      <Button variant="destructive" onclick={confirmToggle}>Remove anyway</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
