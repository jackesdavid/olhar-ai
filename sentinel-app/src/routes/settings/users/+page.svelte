<script lang="ts">
  import PageHeader from "$lib/components/page-header.svelte";
  import { toast } from "svelte-sonner";

  import Busy from "$lib/components/busy.svelte";
  import UserPlus from "@lucide/svelte/icons/user-plus";
  import UsersRound from "@lucide/svelte/icons/users-round";
  import EmptyState from "$lib/components/empty-state.svelte";

  import * as Card from "$lib/components/ui/card";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Badge } from "$lib/components/ui/badge";
  import Loader from "$lib/components/loader.svelte";

  import { rbac, type Role, type TenantUser } from "$lib/api";
  import { hasPerm } from "$lib/auth.svelte";

  let loading = $state(true);
  let users = $state<TenantUser[]>([]);
  let roles = $state<Role[]>([]);
  let savingUser = $state<string | null>(null);

  let inviteEmail = $state("");
  let invitePassword = $state("");
  let inviteRoles = $state<Set<string>>(new Set());
  let inviting = $state(false);

  const allowed = hasPerm("users.manage");

  async function load() {
    loading = true;
    try {
      [users, roles] = await Promise.all([rbac.users(), rbac.roles()]);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Failed to load users");
    } finally {
      loading = false;
    }
  }
  $effect(() => {
    if (allowed) load();
    else loading = false;
  });

  function roleName(id: string): string {
    return roles.find((r) => r.id === id)?.name ?? "role";
  }

  async function toggleRole(user: TenantUser, roleId: string) {
    const next = new Set(user.role_ids);
    if (next.has(roleId)) next.delete(roleId);
    else next.add(roleId);
    user.role_ids = [...next];
    users = [...users];
    savingUser = user.id;
    try {
      await rbac.setUserRoles(user.id, user.role_ids);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Failed to update roles");
      await load();
    } finally {
      savingUser = null;
    }
  }

  function toggleInviteRole(roleId: string) {
    const next = new Set(inviteRoles);
    if (next.has(roleId)) next.delete(roleId);
    else next.add(roleId);
    inviteRoles = next;
  }

  async function invite(e: SubmitEvent) {
    e.preventDefault();
    if (inviting) return;
    inviting = true;
    try {
      await rbac.inviteUser(inviteEmail.trim(), invitePassword, [...inviteRoles]);
      inviteEmail = "";
      invitePassword = "";
      inviteRoles = new Set();
      await load();
      toast.success("User invited");
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Failed to invite user");
    } finally {
      inviting = false;
    }
  }

  const canInvite = $derived(
    inviteEmail.trim().includes("@") && invitePassword.length >= 8,
  );
</script>

<div class="space-y-6">
  <PageHeader
    title="Users"
    description="Invite people into this workspace and assign their roles."
    icon={UsersRound}
  />

  {#if !allowed}
    <Card.Root><Card.Content class="py-10 text-center text-sm text-muted-foreground">
      You don't have permission to manage users.
    </Card.Content></Card.Root>
  {:else if loading}
    <Loader />
  {:else}
    <Card.Root>
      <Card.Header><Card.Title>Members</Card.Title></Card.Header>
      <Card.Content class="overflow-x-auto">
        <table class="w-full border-collapse text-sm">
          <thead>
            <tr class="border-b border-border">
              <th class="py-2 pr-4 text-left font-medium">Email</th>
              {#each roles as role (role.id)}
                <th class="px-3 py-2 text-center font-medium">{role.name}</th>
              {/each}
            </tr>
          </thead>
          <tbody>
            {#each users as user (user.id)}
              <tr class="border-b border-border/50 last:border-0">
                <td class="py-2 pr-4">
                  <div class="flex items-center gap-2">
                    <span>{user.email}</span>
                    {#if user.is_master}<Badge variant="secondary" class="text-[10px]">master</Badge>{/if}
                    {#if savingUser === user.id}<Busy size={12} class="text-muted-foreground" />{/if}
                  </div>
                </td>
                {#each roles as role (role.id)}
                  <td class="px-3 py-2 text-center">
                    <input
                      type="checkbox"
                      class="size-4 cursor-pointer accent-primary"
                      checked={user.role_ids.includes(role.id)}
                      onchange={() => toggleRole(user, role.id)}
                      aria-label={`${user.email}: ${role.name}`}
                    />
                  </td>
                {/each}
              </tr>
            {/each}
            {#if users.length === 0}
              <tr><td colspan={roles.length + 1}>
                <EmptyState
                  title="No users yet"
                  description="Invite teammates below — they sign in with their email and the roles you assign."
                />
              </td></tr>
            {/if}
          </tbody>
        </table>
      </Card.Content>
    </Card.Root>

    <Card.Root>
      <Card.Header>
        <Card.Title>Invite a user</Card.Title>
        <Card.Description>They sign in with this email and password.</Card.Description>
      </Card.Header>
      <Card.Content>
        <form class="space-y-4" onsubmit={invite}>
          <div class="grid gap-4 sm:grid-cols-2">
            <div class="space-y-1.5">
              <Label for="inv-email">Email</Label>
              <Input id="inv-email" type="email" bind:value={inviteEmail} placeholder="teammate@example.com" />
            </div>
            <div class="space-y-1.5">
              <Label for="inv-pw">Temporary password</Label>
              <Input id="inv-pw" type="password" bind:value={invitePassword} placeholder="At least 8 characters" />
            </div>
          </div>
          <div class="space-y-1.5">
            <Label>Roles</Label>
            <div class="flex flex-wrap gap-2">
              {#each roles as role (role.id)}
                <label class="flex cursor-pointer items-center gap-1.5 rounded-md border border-border px-2.5 py-1 text-sm">
                  <input type="checkbox" class="size-3.5 accent-primary" checked={inviteRoles.has(role.id)} onchange={() => toggleInviteRole(role.id)} />
                  {role.name}
                </label>
              {/each}
            </div>
          </div>
          <Button type="submit" disabled={!canInvite || inviting}>
            {#if inviting}<Busy />{:else}<UserPlus class="size-4" />{/if}
            Invite
          </Button>
        </form>
      </Card.Content>
    </Card.Root>
  {/if}
</div>
