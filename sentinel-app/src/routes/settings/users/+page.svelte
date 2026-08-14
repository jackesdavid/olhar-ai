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
  import Link2 from "@lucide/svelte/icons/link-2";
  import QRCode from "qrcode";

  import { rbac, type Role, type TenantUser } from "$lib/api";
  import { API_BASE } from "$lib/config";
  import { auth, hasPerm } from "$lib/auth.svelte";
  import { dict, lang } from "$lib/i18n";

  const t = $derived(dict[lang()].users);

  let loading = $state(true);
  let users = $state<TenantUser[]>([]);
  let roles = $state<Role[]>([]);
  let savingUser = $state<string | null>(null);

  let inviteEmail = $state("");
  let invitePassword = $state("");
  let inviteRoles = $state<Set<string>>(new Set());
  let inviting = $state(false);

  // ---- invite link + QR (#181) --------------------------------------------
  // The token comes back exactly once (it is stored hashed), so everything the admin needs has to
  // be on screen now — link, QR and typed code together.
  let invite_ = $state<{ token: string; code: string; expires_at: string } | null>(null);
  let inviteQr = $state<string | null>(null);
  let makingLink = $state(false);
  let copied = $state(false);

  /** The link a person opens in a browser. */
  function inviteLink(token: string): string {
    return `${window.location.origin}/join/${token}`;
  }

  /** What the QR encodes.
   *
   *  JSON with the CONTROL URL in it, not a bare token: one app binary has to serve any
   *  deployment, and the mobile defaults are a compiled constant that has already pointed at a
   *  host which does not resolve. Letting the invite carry the address removes that whole class
   *  of "the app reaches nothing". */
  function qrPayload(token: string): string {
    return JSON.stringify({ v: 1, url: API_BASE, t: token });
  }

  async function createLink() {
    if (makingLink) return;
    makingLink = true;
    try {
      const email = inviteEmail.trim();
      const r = await rbac.createInvite({
        email: email.includes("@") ? email : undefined,
        roleIds: [...inviteRoles],
      });
      invite_ = { token: r.token, code: r.code, expires_at: r.expires_at };
      inviteQr = await QRCode.toDataURL(qrPayload(r.token), { margin: 1, width: 220 });
      copied = false;
    } catch (e) {
      toast.error(e instanceof Error ? e.message : t.inviteLinkFailed);
    } finally {
      makingLink = false;
    }
  }

  async function copyLink() {
    if (!invite_) return;
    await navigator.clipboard.writeText(inviteLink(invite_.token));
    copied = true;
    toast.success(t.copied);
  }

  // The owner can always reach this screen: control allows the user list for the owner even
  // without users.manage, because every owner action lives here.
  const iAmOwner = $derived(
    users.some((u) => u.id === auth.me?.user_id && u.is_owner),
  );
  // The SERVER decides who may see this screen — it admits the owner even without users.manage.
  // Deriving that here is circular: ownership is only knowable from the user list, and the list
  // would only load if we already believed we were allowed. So: always attempt the load, and let
  // the response settle it.
  let allowed = $state(true);

  // One row at a time may be mid-action. Inline rather than a modal: a confirm dialog for a
  // one-click action is heavier than the action itself.
  let pendingRemove = $state<string | null>(null);
  let pendingTransfer = $state<string | null>(null);
  let pwOpenFor = $state<string | null>(null);
  let newPassword = $state("");
  let acting = $state(false);

  /** Whether the signed-in user may act on `user` at all. Mirrors the server's guards so the UI
   *  doesn't offer buttons that would be refused. */
  function canAct(user: TenantUser): boolean {
    if (user.is_master) return false;
    if (user.id === auth.me?.user_id) return false;
    return iAmOwner || hasPerm("users.password_reset") || hasPerm("users.remove");
  }
  function canReset(user: TenantUser): boolean {
    if (!canAct(user)) return false;
    if (user.is_owner) return false; // only the owner resets the owner, and that isn't self-service here
    return iAmOwner || hasPerm("users.password_reset");
  }
  function canRemove(user: TenantUser): boolean {
    if (!canAct(user)) return false;
    if (user.is_owner) return false; // the owner is never removable — transfer first
    return iAmOwner || hasPerm("users.remove");
  }
  /** Transfer is owner-only and deliberately not delegatable. */
  function canMakeOwner(user: TenantUser): boolean {
    return iAmOwner && !user.is_owner && !user.is_master && user.id !== auth.me?.user_id;
  }

  function closeRowActions() {
    pendingRemove = null;
    pendingTransfer = null;
    pwOpenFor = null;
    newPassword = "";
  }

  async function submitPassword(user: TenantUser) {
    if (newPassword.length < 8 || acting) return;
    acting = true;
    try {
      await rbac.setUserPassword(user.id, newPassword);
      toast.success(t.resetDone);
      closeRowActions();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : t.resetFailed);
    } finally {
      acting = false;
    }
  }

  async function confirmRemove(user: TenantUser) {
    if (acting) return;
    acting = true;
    try {
      await rbac.removeUser(user.id);
      toast.success(t.removeDone);
      closeRowActions();
      await load();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : t.removeFailed);
    } finally {
      acting = false;
    }
  }

  async function confirmTransfer(user: TenantUser) {
    if (acting) return;
    acting = true;
    try {
      await rbac.transferOwnership(user.id);
      toast.success(t.transferDone);
      closeRowActions();
      await load();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : t.transferFailed);
    } finally {
      acting = false;
    }
  }

  async function load() {
    loading = true;
    try {
      [users, roles] = await Promise.all([rbac.users(), rbac.roles()]);
      allowed = true;
    } catch (e) {
      // Refused means neither owner nor users.manage — show the not-permitted state rather than an
      // error toast. With the permission, a failure here is a real error worth surfacing.
      if (hasPerm("users.manage")) {
        toast.error(e instanceof Error ? e.message : t.loadFailed);
      } else {
        allowed = false;
      }
    } finally {
      loading = false;
    }
  }
  $effect(() => {
    load();
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
      toast.error(e instanceof Error ? e.message : t.rolesFailed);
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
      toast.success(t.invited);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : t.inviteFailed);
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
    title={t.title}
    description={t.description}
    icon={UsersRound}
  />

  {#if !allowed}
    <Card.Root><Card.Content class="py-10 text-center text-sm text-muted-foreground">
      {t.noPermission}
    </Card.Content></Card.Root>
  {:else if loading}
    <Loader />
  {:else}
    <Card.Root>
      <Card.Header><Card.Title>{t.members}</Card.Title></Card.Header>
      <Card.Content class="overflow-x-auto">
        <table class="w-full border-collapse text-sm">
          <thead>
            <tr class="border-b border-border">
              <th class="py-2 pr-4 text-left font-medium">{t.email}</th>
              {#each roles as role (role.id)}
                <th class="px-3 py-2 text-center font-medium">{role.name}</th>
              {/each}
              <th class="py-2 pl-3 text-right font-medium">{t.actions}</th>
            </tr>
          </thead>
          <tbody>
            {#each users as user (user.id)}
              <tr class="border-b border-border/50 last:border-0">
                <td class="py-2 pr-4">
                  <div class="flex items-center gap-2">
                    <span>{user.email}</span>
                    {#if user.is_master}<Badge variant="secondary" class="text-[10px]">{t.master}</Badge>{/if}
                    {#if user.is_owner}<Badge variant="outline" class="text-[10px]" title={t.ownerHint}>{t.ownerBadge}</Badge>{/if}
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
                <td class="py-2 pl-3 text-right">
                  {#if pwOpenFor === user.id}
                    <div class="flex items-center justify-end gap-1.5">
                      <Input
                        type="password"
                        class="h-7 w-44 text-xs"
                        placeholder={t.newPassword}
                        autocomplete="new-password"
                        bind:value={newPassword}
                      />
                      <Button size="sm" variant="default" class="h-7 text-xs" disabled={newPassword.length < 8 || acting} onclick={() => submitPassword(user)}>
                        {t.resetConfirm}
                      </Button>
                      <Button size="sm" variant="ghost" class="h-7 text-xs" onclick={closeRowActions}>{t.cancel}</Button>
                    </div>
                  {:else if pendingRemove === user.id}
                    <div class="flex items-center justify-end gap-1.5">
                      <Button size="sm" variant="destructive" class="h-7 text-xs" disabled={acting} onclick={() => confirmRemove(user)}>
                        {t.removeConfirm}
                      </Button>
                      <Button size="sm" variant="ghost" class="h-7 text-xs" onclick={closeRowActions}>{t.cancel}</Button>
                    </div>
                  {:else if pendingTransfer === user.id}
                    <div class="flex items-center justify-end gap-1.5">
                      <Button size="sm" variant="default" class="h-7 text-xs" disabled={acting} onclick={() => confirmTransfer(user)}>
                        {t.transferConfirm}
                      </Button>
                      <Button size="sm" variant="ghost" class="h-7 text-xs" onclick={closeRowActions}>{t.cancel}</Button>
                    </div>
                  {:else}
                    <div class="flex items-center justify-end gap-1">
                      {#if canReset(user)}
                        <Button size="sm" variant="ghost" class="h-7 text-xs" onclick={() => { closeRowActions(); pwOpenFor = user.id; }}>
                          {t.resetPassword}
                        </Button>
                      {/if}
                      {#if canMakeOwner(user)}
                        <Button size="sm" variant="ghost" class="h-7 text-xs" onclick={() => { closeRowActions(); pendingTransfer = user.id; }}>
                          {t.makeOwner}
                        </Button>
                      {/if}
                      {#if canRemove(user)}
                        <Button size="sm" variant="ghost" class="h-7 text-xs text-destructive" onclick={() => { closeRowActions(); pendingRemove = user.id; }}>
                          {t.remove}
                        </Button>
                      {/if}
                    </div>
                  {/if}
                </td>
              </tr>
            {/each}
            {#if users.length === 0}
              <tr><td colspan={roles.length + 2}>
                <EmptyState
                  title={t.emptyTitle}
                  description={t.emptyDescription}
                />
              </td></tr>
            {/if}
          </tbody>
        </table>
      </Card.Content>
    </Card.Root>

    <Card.Root>
      <Card.Header>
        <Card.Title>{t.inviteTitle}</Card.Title>
        <Card.Description>{t.inviteDescription}</Card.Description>
      </Card.Header>
      <Card.Content>
        <form class="space-y-4" onsubmit={invite}>
          <div class="grid gap-4 sm:grid-cols-2">
            <div class="space-y-1.5">
              <Label for="inv-email">{t.email}</Label>
              <Input id="inv-email" type="email" bind:value={inviteEmail} placeholder={t.emailPlaceholder} />
            </div>
            <div class="space-y-1.5">
              <Label for="inv-pw">{t.tempPassword}</Label>
              <Input id="inv-pw" type="password" bind:value={invitePassword} placeholder={t.tempPasswordPlaceholder} autocomplete="new-password" />
            </div>
          </div>
          <div class="space-y-1.5">
            <Label>{t.roles}</Label>
            <div class="flex flex-wrap gap-2">
              {#each roles as role (role.id)}
                <label class="flex cursor-pointer items-center gap-1.5 rounded-md border border-border px-2.5 py-1 text-sm">
                  <input type="checkbox" class="size-3.5 accent-primary" checked={inviteRoles.has(role.id)} onchange={() => toggleInviteRole(role.id)} />
                  {role.name}
                </label>
              {/each}
            </div>
          </div>
          <div class="flex flex-wrap items-center gap-2">
            <Button type="submit" disabled={!canInvite || inviting}>
              {#if inviting}<Busy />{:else}<UserPlus class="size-4" />{/if}
              {t.invite}
            </Button>
            <!-- The link path needs no password: the person sets their own on accept, which is
                 the whole point (#181). Roles come from the checkboxes above either way. -->
            <Button type="button" variant="outline" disabled={makingLink} onclick={createLink}>
              {#if makingLink}<Busy />{:else}<Link2 class="size-4" />{/if}
              {makingLink ? t.creatingLink : t.createLink}
            </Button>
          </div>
        </form>

        {#if invite_ && inviteQr}
          <div class="mt-4 space-y-3 rounded-lg border border-border p-4">
            <div>
              <p class="text-sm font-medium">{t.inviteLinkTitle}</p>
              <p class="text-xs text-muted-foreground">{t.inviteLinkDescription}</p>
            </div>
            <div class="flex flex-wrap items-start gap-4">
              <img src={inviteQr} alt={t.qrAlt} class="size-[220px] rounded bg-white p-2" />
              <div class="min-w-0 flex-1 space-y-3">
                <div class="space-y-1.5">
                  <Input readonly value={inviteLink(invite_.token)} class="font-mono text-xs" />
                  <Button size="sm" variant="secondary" onclick={copyLink}>
                    {copied ? t.copied : t.copyLink}
                  </Button>
                </div>
                <div class="space-y-1">
                  <p class="text-xs text-muted-foreground">{t.inviteCode}</p>
                  <p class="font-mono text-2xl tracking-[0.25em]">{invite_.code}</p>
                </div>
                <p class="text-xs text-muted-foreground">{t.expiresOn(invite_.expires_at.slice(0, 10))}</p>
                <!-- Said plainly because it is true: the token is stored hashed and this is the
                     only time it can be shown. -->
                <p class="text-xs text-muted-foreground">{t.inviteOnce}</p>
              </div>
            </div>
          </div>
        {/if}
      </Card.Content>
    </Card.Root>
  {/if}
</div>
