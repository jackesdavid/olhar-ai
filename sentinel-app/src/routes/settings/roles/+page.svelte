<script lang="ts">
  import PageHeader from "$lib/components/page-header.svelte";
  import { onMount } from "svelte";
  import { toast } from "svelte-sonner";

  import Busy from "$lib/components/busy.svelte";
  import Plus from "@lucide/svelte/icons/plus";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";

  import * as Card from "$lib/components/ui/card";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Badge } from "$lib/components/ui/badge";
  import Loader from "$lib/components/loader.svelte";

  import { rbac, type Permission, type Role } from "$lib/api";
  import { hasPerm } from "$lib/auth.svelte";

  let loading = $state(true);
  let permissions = $state<Permission[]>([]);
  let roles = $state<Role[]>([]);
  // roleId -> set of granted permission keys (the matrix).
  let matrix = $state<Record<string, Set<string>>>({});
  let saving = $state<string | null>(null);

  let newRole = $state("");
  let creating = $state(false);

  const allowed = hasPerm("roles.manage");

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

  function isGranted(roleId: string, key: string): boolean {
    return matrix[roleId]?.has(key) ?? false;
  }

  async function toggle(role: Role, key: string) {
    const set = new Set(matrix[role.id] ?? []);
    if (set.has(key)) set.delete(key);
    else set.add(key);
    matrix = { ...matrix, [role.id]: set };
    saving = role.id;
    try {
      await rbac.setRolePerms(role.id, [...set]);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Failed to save");
      await load(); // re-sync on failure
    } finally {
      saving = null;
    }
  }

  async function createRole(e: SubmitEvent) {
    e.preventDefault();
    const name = newRole.trim();
    if (!name || creating) return;
    creating = true;
    try {
      await rbac.createRole(name);
      newRole = "";
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
    title={"Roles & permissions"}
    description="Toggle which actions each role can perform. Access is granted by action, never by role name."
    icon={ShieldCheck}
  />

  {#if !allowed}
    <Card.Root><Card.Content class="py-10 text-center text-sm text-muted-foreground">
      You don't have permission to manage roles.
    </Card.Content></Card.Root>
  {:else if loading}
    <Loader />
  {:else}
    <Card.Root>
      <Card.Header>
        <Card.Title>Permission matrix</Card.Title>
        <Card.Description>Changes save automatically.</Card.Description>
      </Card.Header>
      <Card.Content class="overflow-x-auto">
        <table class="w-full border-collapse text-sm">
          <thead>
            <tr class="border-b border-border">
              <th class="sticky left-0 bg-card py-2 pr-4 text-left font-medium">Action</th>
              {#each roles as role (role.id)}
                <th class="px-3 py-2 text-center font-medium">
                  <div class="inline-flex items-center gap-1.5">
                    {role.name}
                    {#if role.is_system}<Badge variant="secondary" class="text-[10px]">system</Badge>{/if}
                    {#if saving === role.id}<Busy size={12} class="text-muted-foreground" />{/if}
                  </div>
                </th>
              {/each}
            </tr>
          </thead>
          <tbody>
            {#each permissions as perm (perm.key)}
              <tr class="border-b border-border/50 last:border-0">
                <td class="sticky left-0 bg-card py-2 pr-4">
                  <div class="font-mono text-xs">{perm.key}</div>
                  <div class="text-xs text-muted-foreground">{perm.description}</div>
                </td>
                {#each roles as role (role.id)}
                  <td class="px-3 py-2 text-center">
                    <input
                      type="checkbox"
                      class="size-4 cursor-pointer accent-primary"
                      checked={isGranted(role.id, perm.key)}
                      onchange={() => toggle(role, perm.key)}
                      aria-label={`${role.name}: ${perm.key}`}
                    />
                  </td>
                {/each}
              </tr>
            {/each}
          </tbody>
        </table>
      </Card.Content>
    </Card.Root>

    <Card.Root>
      <Card.Header>
        <Card.Title>New role</Card.Title>
        <Card.Description>Create a custom role, then grant it actions above.</Card.Description>
      </Card.Header>
      <Card.Content>
        <form class="flex gap-2" onsubmit={createRole}>
          <Input bind:value={newRole} placeholder="e.g. Operator" class="max-w-xs" />
          <Button type="submit" disabled={!newRole.trim() || creating}>
            {#if creating}<Busy />{:else}<Plus class="size-4" />{/if}
            Create
          </Button>
        </form>
      </Card.Content>
    </Card.Root>
  {/if}
</div>
